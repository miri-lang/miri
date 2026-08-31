// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri patch` command: edit a program and re-check it in one step.
//!
//! A tool that edits Miri source otherwise runs three steps to make one change:
//! rewrite the text, work out what the rewrite did to the formatting, and check
//! the result. This command is the three of them together. It anchors an edit
//! against the canonical rendering a reader already has, applies it to the
//! bytes the author wrote, and answers with the diagnostics of the edited
//! program — so a caller learns whether its change holds up, not merely whether
//! the substitution happened.
//!
//! Nothing reaches disk that the edit made worse. Every operation is applied to
//! a copy held in memory, the copy is checked once, and only then is the file
//! replaced — but only if the edit introduced no new diagnostics. An edit that
//! introduces new errors is refused and the file is left unchanged. An edit that
//! leaves only pre-existing errors untouched is accepted and written, with those
//! pre-existing errors reported and marked as such.
//!
//! What is re-checked is the target file and the modules it imports. A file
//! that imports the patched one is not re-checked here.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::ast::formatter;
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::cli::{
    resolve, sanitize_for_terminal, serialize_envelope, token_align, ColorMode, Format,
};
use crate::diagnostics::json::{
    DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonPatch, JsonPatchEdit,
};
use crate::diagnostics::{DiagnosticCode, Severity};
use crate::error::diagnostic::{to_json, Diagnostic, DiagnosticBuilder, Reportable};
use crate::error::format::format_diagnostic_with_color;
use crate::error::type_error::TypeError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::pipeline::Pipeline;

mod insertion;
mod request;

pub use request::{operations, Request};

use insertion::{
    already_declared, confirm_text_declares_only, dominant_line_ending, indented, place,
};

/// What one operation does to one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Replace one occurrence of some text in the function's canonical form.
    Anchored {
        /// The text to find, matched against the canonical rendering.
        old: String,
        /// What to put in its place.
        new: String,
    },
    /// Replace everything past the function's header.
    Body {
        /// The replacement body, as the caller wrote it.
        text: String,
    },
    /// Add a new declaration the file does not have.
    Insert {
        /// The declaration text, as the caller wrote it.
        text: String,
        /// The declaration the new one follows; `None` appends.
        after: Option<String>,
    },
}

/// One edit, and the function it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// The function's name, or `Class.method` for a method.
    /// For Anchored and Body edits: the target to modify.
    /// For Insert edits: the new declaration to create.
    pub function: String,
    /// What to do to it.
    pub edit: Edit,
}

/// Whether the edited program is allowed to reach disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Write the edited program when it checks.
    Apply,
    /// Check the edited program and report, writing nothing.
    CheckOnly,
    /// Report the difference the edit would make, writing nothing.
    DryRun,
}

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The edits applied and the edited program checked.
    Succeeded,
    /// The request could not be answered, or the edited program did not check.
    Failed,
}

/// What a patch did.
pub struct PatchReport {
    /// The envelope, ready to serialize for a machine consumer.
    pub envelope: DiagnosticsEnvelope,
    /// Whether the file on disk was replaced.
    pub file_was_written: bool,
    /// The difference the edits make, present only for a dry run.
    pub diff: Option<String>,
    /// The diagnostics as the compiler reported them.
    diagnostics: Vec<Diagnostic>,
    /// The source the diagnostics were reported against.
    source: String,
    /// The path the diagnostics were reported against.
    source_path: Option<String>,
}

impl PatchReport {
    /// Render the diagnostics for a person to read.
    pub fn to_pretty(&self, color_mode: ColorMode) -> String {
        self.diagnostics
            .iter()
            .map(|diagnostic| {
                format_diagnostic_with_color(
                    &self.source,
                    diagnostic,
                    self.source_path.as_deref(),
                    color_mode.into(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Result of validating the source and applying operations.
struct ValidatedEdit {
    /// Original source file content.
    source: String,
    /// Edited source file content.
    edited: String,
    /// The edits that were applied.
    applied: Vec<JsonPatchEdit>,
    /// Check result of the edited source.
    checked_edited: Checked,
}

/// Validate source file and apply operations; return early if any step fails.
fn validate_and_apply(
    path: &Path,
    operations: &[Operation],
    expect_sha: Option<&str>,
) -> Result<ValidatedEdit, Box<PatchReport>> {
    let source_path = Some(path.display().to_string());
    let source = fs::read_to_string(path).map_err(|error| {
        Box::new(refusal(
            vec![unreadable(path, &error)],
            String::new(),
            source_path.clone(),
            0,
        ))
    })?;

    if let Some(diagnostic) = request_refused(&source, operations, expect_sha) {
        return Err(Box::new(refusal(vec![diagnostic], source, source_path, 0)));
    }

    let (edited, applied) = fold_operations(&source, operations).map_err(|diagnostic| {
        Box::new(refusal(
            vec![*diagnostic],
            source.clone(),
            source_path.clone(),
            0,
        ))
    })?;

    let checked_edited = check_source(path, &edited);

    Ok(ValidatedEdit {
        source,
        edited,
        applied,
        checked_edited,
    })
}

/// Apply `operations` to `path` and report what happened.
///
/// Nothing here writes to a stream or ends the process, so the same call serves
/// the command line and a request over a long-lived connection.
pub fn patch(
    path: &Path,
    operations: &[Operation],
    expect_sha: Option<&str>,
    mode: Mode,
) -> PatchReport {
    let source_path = Some(path.display().to_string());
    let validated = match validate_and_apply(path, operations, expect_sha) {
        Ok(v) => v,
        Err(report) => return *report,
    };

    if validated.checked_edited.ok {
        let file_was_written = match store(path, &validated.edited, mode) {
            Ok(written) => written,
            Err(diagnostic) => return refusal(vec![*diagnostic], validated.source, source_path, 1),
        };
        return accepted_report(
            Applied {
                edits: validated.applied,
                warnings: validated.checked_edited.warnings,
                file_was_written,
            },
            Texts {
                before: validated.source.clone(),
                after: validated.edited,
                path: source_path,
            },
            matches!(mode, Mode::DryRun).then_some(path),
            PreexistingErrors::none(),
        );
    }

    let checked_baseline = check_source(path, &validated.source);
    let partitioned = partition_errors_against_baseline(
        &checked_baseline.errors,
        &validated.checked_edited.errors,
        &validated.source,
        &validated.edited,
        source_path.as_deref(),
    );

    if !partitioned.new_errors.is_empty() {
        return reject_with_errors(
            validated.edited,
            source_path,
            partitioned,
            &validated.checked_edited,
        );
    }

    let file_was_written = match store(path, &validated.edited, mode) {
        Ok(written) => written,
        Err(diagnostic) => return refusal(vec![*diagnostic], validated.source, source_path, 1),
    };

    accepted_report(
        Applied {
            edits: validated.applied,
            warnings: validated.checked_edited.warnings,
            file_was_written,
        },
        Texts {
            before: validated.source,
            after: validated.edited,
            path: source_path,
        },
        matches!(mode, Mode::DryRun).then_some(path),
        partitioned.preexisting,
    )
}

/// Report a file the command was asked to read and could not.
fn unreadable(path: &Path, error: &std::io::Error) -> Diagnostic {
    coded(
        DiagnosticCode::BldInputNotReadable,
        format!("could not read {}: {}", path.display(), error),
        "check the path exists, names a file rather than a directory, and is readable",
    )
}

/// Put the edited text on disk, unless the mode says to hold it back.
fn store(path: &Path, edited: &str, mode: Mode) -> Result<bool, Box<Diagnostic>> {
    match mode {
        Mode::Apply => write_atomically(path, edited)
            .map(|()| true)
            .map_err(|error| {
                Box::new(coded(
                    DiagnosticCode::BldInputNotReadable,
                    format!("could not write {}: {}", path.display(), error),
                    "check the file is writable and its directory has space",
                ))
            }),
        Mode::CheckOnly | Mode::DryRun => Ok(false),
    }
}

/// What a successful batch produced.
struct Applied {
    /// The edits, in the order they were applied.
    edits: Vec<JsonPatchEdit>,
    /// The warnings the edited program's check reported.
    warnings: Vec<Diagnostic>,
    /// Whether the file on disk was replaced.
    file_was_written: bool,
}

/// The texts a report is written against.
struct Texts {
    /// The file as it was before the edits.
    before: String,
    /// The file as the edits leave it.
    after: String,
    /// The path the diagnostics were reported against.
    path: Option<String>,
}

/// The errors an edit left in place, in both renderings.
struct PreexistingErrors {
    /// The errors as a machine consumer receives them.
    json: Vec<JsonDiagnostic>,
    /// The same errors as the compiler reported them, for rendering.
    diagnostics: Vec<Diagnostic>,
}

impl PreexistingErrors {
    /// The empty set, for an edit over a file that already checked.
    fn none() -> Self {
        PreexistingErrors {
            json: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// Errors partitioned against a baseline.
struct PartitionedErrors {
    /// Errors that did not exist before the edit.
    new_errors: Vec<JsonDiagnostic>,
    /// Errors that existed before the edit.
    preexisting: PreexistingErrors,
}

/// The parts every report is assembled from.
struct ReportParts {
    /// Whether the request was answered.
    ok: bool,
    /// The diagnostics as a machine consumer receives them.
    json_diagnostics: Vec<JsonDiagnostic>,
    /// The edits that were applied, empty when nothing was.
    edits: Vec<JsonPatchEdit>,
    /// How many times the edited program was checked.
    revalidations: u32,
    /// Whether the file on disk was replaced.
    file_written: bool,
    /// The difference the edits make, present only for a dry run.
    diff: Option<String>,
    /// The diagnostics as the compiler reported them.
    diagnostics: Vec<Diagnostic>,
    /// The text the diagnostics are rendered against.
    source: String,
    /// The path the diagnostics were reported against.
    source_path: Option<String>,
}

/// Assemble a report from its parts.
///
/// Every exit from the command comes through here, so the exit code and the
/// `ok` flag cannot disagree and `fileWritten` cannot drift from what the
/// report says about the file.
fn report(parts: ReportParts) -> PatchReport {
    let exit_code = if parts.ok { 0 } else { 1 };
    PatchReport {
        envelope: DiagnosticsEnvelope::new(JsonCommand::Patch, parts.ok, parts.json_diagnostics)
            .with_exit_code(exit_code)
            .with_patch(JsonPatch {
                edits: parts.edits,
                revalidations: parts.revalidations,
                file_written: parts.file_written,
            }),
        file_was_written: parts.file_written,
        diff: parts.diff,
        diagnostics: parts.diagnostics,
        source: parts.source,
        source_path: parts.source_path,
    }
}

/// Build the report for a batch that applied, together with any pre-existing
/// errors the edit left in place.
///
/// An edit over a clean file is the same report with nothing pre-existing, so
/// both outcomes share one builder.
fn accepted_report(
    applied: Applied,
    texts: Texts,
    diff_for: Option<&Path>,
    preexisting: PreexistingErrors,
) -> PatchReport {
    let mut json_diagnostics = preexisting.json;
    for diagnostic in json_diagnostics.iter_mut() {
        diagnostic.preexisting = Some(true);
    }
    json_diagnostics.extend(
        applied
            .warnings
            .iter()
            .map(|diagnostic| to_json(diagnostic, &texts.after, texts.path.as_deref())),
    );

    let mut diagnostics = preexisting.diagnostics;
    diagnostics.extend(applied.warnings);

    report(ReportParts {
        ok: true,
        json_diagnostics,
        edits: applied.edits,
        revalidations: 1,
        file_written: applied.file_was_written,
        diff: diff_for.map(|path| unified_diff(path, &texts.before, &texts.after)),
        diagnostics,
        source: texts.after,
        source_path: texts.path,
    })
}

/// Refuse a request that names nothing to do, or that was prepared against a
/// file that has since moved on.
fn request_refused(
    source: &str,
    operations: &[Operation],
    expect_sha: Option<&str>,
) -> Option<Diagnostic> {
    if let Some(expected) = expect_sha {
        if let Some(diagnostic) = stale_hash(source, expected) {
            return Some(diagnostic);
        }
    }
    if operations.is_empty() {
        return Some(coded(
            DiagnosticCode::BldMalformedEditRequest,
            "no edit was requested".to_string(),
            "name an edit with --replace-in-fn together with --old and --new, or with --replace-fn together with --body-file",
        ));
    }
    None
}

/// Apply every operation to one text, in order.
///
/// Each operation sees what the ones before it did, so an anchor may name text
/// an earlier edit introduced.
fn fold_operations(
    source: &str,
    operations: &[Operation],
) -> Result<(String, Vec<JsonPatchEdit>), Box<Diagnostic>> {
    let mut edited = source.to_string();
    let mut applied = Vec::new();
    for operation in operations {
        let (next, edit) = apply_one(&edited, operation)?;
        edited = next;
        applied.push(edit);
    }
    Ok((edited, applied))
}

/// Reject an edit that introduced new errors, reporting every diagnostic of the
/// edited program.
///
/// Positions are the edited program's, and so is the text they are rendered
/// against, even though that text never reached disk.
fn reject_with_errors(
    source_for_rendering: String,
    source_path: Option<String>,
    partitioned: PartitionedErrors,
    checked_edited: &Checked,
) -> PatchReport {
    let notice = rejected_edit();
    let as_json = |diagnostic: &Diagnostic| {
        to_json(diagnostic, &source_for_rendering, source_path.as_deref())
    };

    let mut json_diagnostics = vec![as_json(&notice)];
    for mut new_error in partitioned.new_errors {
        new_error.preexisting = Some(false);
        json_diagnostics.push(new_error);
    }
    for mut old_error in partitioned.preexisting.json {
        old_error.preexisting = Some(true);
        json_diagnostics.push(old_error);
    }
    json_diagnostics.extend(checked_edited.warnings.iter().map(as_json));

    let mut diagnostics = vec![notice];
    diagnostics.extend(checked_edited.warnings.iter().cloned());
    diagnostics.extend(checked_edited.errors.iter().cloned());

    report(ReportParts {
        ok: false,
        json_diagnostics,
        edits: Vec::new(),
        revalidations: 1,
        file_written: false,
        diff: None,
        diagnostics,
        source: source_for_rendering,
        source_path,
    })
}

/// Partition edited errors into new vs. pre-existing, comparing against baseline.
fn partition_errors_against_baseline(
    baseline_errors: &[Diagnostic],
    edited_errors: &[Diagnostic],
    source_before: &str,
    source_after: &str,
    source_path: Option<&str>,
) -> PartitionedErrors {
    let baseline: HashSet<_> = baseline_errors
        .iter()
        .map(|diagnostic| to_json(diagnostic, source_before, source_path))
        .map(|json| diagnostic_fingerprint(&json, false))
        .collect();

    let line_map = build_line_map(source_before, source_after);

    let mut new_errors = Vec::new();
    let mut preexisting = PreexistingErrors::none();

    for diagnostic in edited_errors {
        let json_diagnostic = to_json(diagnostic, source_after, source_path);
        let (is_in_changed_run, mapped) =
            map_diagnostic_line_with_status(&json_diagnostic, &line_map, source_path);
        let fingerprint = diagnostic_fingerprint(&mapped, is_in_changed_run);

        if baseline.contains(&fingerprint) {
            // The mapped copy serves the comparison only. What is reported is
            // the unmapped one, whose line is the edited file's.
            preexisting.json.push(json_diagnostic);
            preexisting.diagnostics.push(diagnostic.clone());
        } else {
            new_errors.push(json_diagnostic);
        }
    }

    PartitionedErrors {
        new_errors,
        preexisting,
    }
}

/// Apply one operation to `source`, returning the edited text and what it did.
fn apply_one(
    source: &str,
    operation: &Operation,
) -> Result<(String, JsonPatchEdit), Box<Diagnostic>> {
    match &operation.edit {
        Edit::Anchored { old, new } => {
            apply_in_place(source, operation, InPlace::Anchored { old, new })
        }
        Edit::Body { text } => apply_in_place(source, operation, InPlace::Body { text }),
        Edit::Insert { text, after } => apply_insert(source, operation, text, after.as_deref()),
    }
}

/// An edit that rewrites bytes inside a declaration that already exists.
///
/// Insert is absent by construction: it names a declaration the file does not
/// have yet, so the resolution this path opens with would refuse it, and the
/// refusal would describe the wrong problem.
enum InPlace<'edit> {
    /// Replace one occurrence of some text in the declaration.
    Anchored {
        /// The text to find, matched against the canonical rendering.
        old: &'edit str,
        /// What to put in its place.
        new: &'edit str,
    },
    /// Replace everything past the declaration's header.
    Body {
        /// The replacement body, as the caller wrote it.
        text: &'edit str,
    },
}

/// Rewrite bytes inside a declaration the file already holds.
fn apply_in_place(
    source: &str,
    operation: &Operation,
    edit: InPlace<'_>,
) -> Result<(String, JsonPatchEdit), Box<Diagnostic>> {
    let program = parse(source)?;
    let declaration = resolve::resolve(&program, &operation.function)?;
    let StatementKind::FunctionDeclaration(data) = &declaration.node else {
        return Err(not_a_function(&operation.function));
    };

    let rendered = formatter::declaration(declaration);
    let alignment =
        token_align::build_alignment(source, &rendered.text, &data.name, data.name_span)
            .map_err(|diverged| Box::new(diverged.to_diagnostic()))?;

    let (range, replacement) = match edit {
        InPlace::Anchored { old, new } => {
            let canonical = anchor_range(&rendered.text, old)?;
            let range = alignment
                .raw_range(canonical.0, canonical.1)
                .ok_or_else(|| anchor_covers_no_token(old))?;
            (range, reindented(source, range.0, new))
        }
        InPlace::Body { text } => {
            let header = formatter::signature(declaration)
                .and_then(|signature| token_align::significant_token_count(&signature.text))
                .ok_or_else(|| not_a_function(&operation.function))?;
            let range = alignment
                .raw_body_range(header)
                .ok_or_else(|| headerless_body(&operation.function))?;
            (range, reindented(source, range.0, text))
        }
    };

    let mut edited = String::with_capacity(source.len() + replacement.len());
    edited.push_str(source.get(..range.0).ok_or_else(split_failed)?);
    edited.push_str(&replacement);
    edited.push_str(source.get(range.1..).ok_or_else(split_failed)?);

    confirm_only_target_changed(&edited, declaration, &program)?;

    Ok((
        edited,
        JsonPatchEdit {
            start: range.0,
            end: range.1,
            replacement,
        },
    ))
}

/// Add a declaration the file does not have.
///
/// The text is spliced in as the caller wrote it, re-indented to sit where it
/// lands. Nothing else in the file moves, and the edited program is checked
/// before any of it reaches disk, so a declaration that does not belong where
/// it was asked for is refused rather than written.
fn apply_insert(
    source: &str,
    operation: &Operation,
    text: &str,
    after: Option<&str>,
) -> Result<(String, JsonPatchEdit), Box<Diagnostic>> {
    let program = parse(source)?;
    let container = operation.function.rsplit_once('.').map(|(name, _)| name);

    if already_declared(&program, &operation.function) {
        return Err(already_exists(&operation.function));
    }
    confirm_text_declares_only(text, &operation.function)?;

    let placement = place(source, &program, container, after)?;
    let ending = dominant_line_ending(source);
    let body = indented(text, &placement.indent, ending);
    let replacement = if placement.separated {
        format!("{}{}{}", ending, ending, body)
    } else {
        body
    };

    let mut edited = String::with_capacity(source.len() + replacement.len());
    edited.push_str(source.get(..placement.at).ok_or_else(split_failed)?);
    edited.push_str(&replacement);
    edited.push_str(source.get(placement.at..).ok_or_else(split_failed)?);

    confirm_insert(&edited, operation, &program, container)?;

    Ok((
        edited,
        JsonPatchEdit {
            start: placement.at,
            end: placement.at,
            replacement,
        },
    ))
}

/// Check that the insert added the one declaration it named and nothing else.
///
/// An insert and an in-place edit fail in different shapes, which is why this
/// does not go through [`confirm_only_target_changed`]: that one holds the
/// file's declaration count fixed, and an insert is the case that changes it.
/// The two insert paths differ again from each other. A top-level declaration
/// makes the file declare one more thing, so setting the new one aside has to
/// leave exactly the sequence that was there. A method leaves the count alone
/// and grows its container instead, so what is checked is that the container
/// is the only declaration whose rendering moved.
fn confirm_insert(
    after: &str,
    operation: &Operation,
    parsed_before: &Program,
    container: Option<&str>,
) -> Result<(), Box<Diagnostic>> {
    let reparsed = parse(after)?;
    match container {
        Some(name) => confirm_method_arrived(&reparsed, parsed_before, &operation.function, name),
        None => confirm_declaration_arrived(&reparsed, parsed_before, &operation.function),
    }
}

/// Check a method joined its container and left every other declaration alone.
///
/// A method changes only the container that holds it, so the file declares
/// exactly what it declared before.
fn confirm_method_arrived(
    reparsed: &Program,
    parsed_before: &Program,
    name: &str,
    container: &str,
) -> Result<(), Box<Diagnostic>> {
    if reparsed.body.len() != parsed_before.body.len() {
        return Err(edit_escaped());
    }
    confirm_untouched(parsed_before, reparsed, |statement| {
        resolve::container_name(statement).as_deref() == Some(container)
    })?;

    let Some((_, method)) = name.rsplit_once('.') else {
        return Err(declared_something_else(name));
    };
    if resolve::methods_of(reparsed, container, method).is_empty() {
        return Err(declared_something_else(name));
    }
    Ok(())
}

/// Check a top-level declaration arrived and displaced nothing.
///
/// The file declares one more thing than it did, and setting the new one aside
/// leaves the sequence that was there before.
fn confirm_declaration_arrived(
    reparsed: &Program,
    parsed_before: &Program,
    name: &str,
) -> Result<(), Box<Diagnostic>> {
    if reparsed.body.len() != parsed_before.body.len() + 1 {
        return Err(edit_escaped());
    }
    let inserted = reparsed
        .body
        .iter()
        .position(|statement| resolve::declared_function_name(statement) == Some(name))
        .ok_or_else(|| declared_something_else(name))?;

    let surviving = reparsed
        .body
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != inserted)
        .map(|(_, statement)| statement);
    for (old, new) in parsed_before.body.iter().zip(surviving) {
        if formatter::declaration(old).text != formatter::declaration(new).text {
            return Err(edit_escaped());
        }
    }
    Ok(())
}

/// Check every declaration the edit did not name renders as it did before.
fn confirm_untouched(
    before: &Program,
    after: &Program,
    is_target: impl Fn(&Statement) -> bool,
) -> Result<(), Box<Diagnostic>> {
    for (old, new) in before.body.iter().zip(&after.body) {
        if is_target(old) {
            continue;
        }
        if formatter::declaration(old).text != formatter::declaration(new).text {
            return Err(edit_escaped());
        }
    }
    Ok(())
}

/// Report a name the file already declares where the new one would go.
fn already_exists(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldDeclarationAlreadyExists,
        format!(
            "`{}` is already declared in this file",
            sanitize_for_terminal(name)
        ),
        "an insert adds a declaration the file does not have; edit the existing one with --replace-fn, or insert under a name nothing answers to",
    ))
}

/// Report a method addressed to a container the file does not declare.
fn container_missing(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldFunctionNotFound,
        format!(
            "no class, struct, enum or trait named `{}` in this file",
            sanitize_for_terminal(name)
        ),
        "a method is inserted into a container the file already declares; run `miri view --outline` to list what it declares",
    ))
}

/// Report a container with no body a member could join.
fn bodyless_container(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        format!(
            "`{}` has no member to place a new one beside",
            sanitize_for_terminal(name)
        ),
        "a method is inserted beside the members a container already declares; add the first one by hand",
    ))
}

/// Report an anchor that does not sit where the new declaration would.
fn anchor_in_another_scope(
    anchor: &str,
    wanted: Option<&str>,
    found: Option<&str>,
) -> Box<Diagnostic> {
    let describe = |container: Option<&str>| match container {
        Some(name) => format!("inside `{}`", sanitize_for_terminal(name)),
        None => "at the top level".to_string(),
    };
    Box::new(coded(
        DiagnosticCode::BldMalformedEditRequest,
        format!(
            "`{}` sits {}, and the new declaration would go {}",
            sanitize_for_terminal(anchor),
            describe(found),
            describe(wanted)
        ),
        "an insert goes beside the declaration --after names, so the two belong to the same container; drop --after to append instead",
    ))
}

/// Report a declaration whose extent could not be established.
fn not_anchorable(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        format!(
            "the extent of `{}` could not be established",
            sanitize_for_terminal(name)
        ),
        "this declaration could not be anchored reliably; rewrite it in canonical form and retry",
    ))
}

/// Report inserted text that declares something other than what was named.
fn declared_something_else(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldMalformedEditRequest,
        format!(
            "the inserted text does not declare `{}`",
            sanitize_for_terminal(name)
        ),
        "the text passed with --body-file must declare the name given to --insert-fn",
    ))
}
/// An anchor or replacement rendered so that the depth it was written at
/// cannot decide whether it matches.
///
/// The first line is taken without its leading whitespace: an anchor may be
/// copied straight out of the file, where it carries the indentation of the
/// line it sat on, or typed flat. The lines below it are dedented by the
/// shallowest indentation among them, which discards the depth the block sits
/// at while keeping the shape of anything nested inside it.
fn without_indentation(text: &str) -> String {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let rest: Vec<&str> = lines.collect();
    let base = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut rendered = String::from(first.trim_start());
    for line in rest {
        rendered.push('\n');
        if !line.trim().is_empty() {
            rendered.push_str(line.get(base..).unwrap_or(line));
        }
    }
    rendered
}

/// Each line of `text` with the byte offset it starts at.
fn line_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut offsets = Vec::new();
    let mut at = 0;
    for line in text.lines() {
        offsets.push((at, line));
        at += line.len() + 1;
    }
    offsets
}

/// Every run of lines in `canonical` that holds `anchor`, ignoring how deeply
/// either was indented.
///
/// A site starts at the first non-blank byte of its opening line rather than at
/// the line break, so the range hands back exactly the bytes the anchor names
/// and the indentation in front of them is left alone.
fn anchor_sites(canonical: &str, anchor: &str) -> Vec<(usize, usize)> {
    let wanted = without_indentation(anchor);
    let span = anchor.lines().count();
    let lines = line_offsets(canonical);
    let mut sites = Vec::new();
    if span == 0 || span > lines.len() {
        return sites;
    }

    for start in 0..=lines.len() - span {
        let window = &lines[start..start + span];
        let text = window
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        if without_indentation(&text) != wanted {
            continue;
        }
        let (first_offset, first_line) = window[0];
        let opens_at = first_offset + (first_line.len() - first_line.trim_start().len());
        let (last_offset, last_line) = window[span - 1];
        sites.push((opens_at, last_offset + last_line.len()));
    }
    sites
}

/// Where an anchor sits in a canonical rendering.
///
/// The anchor has to name one site. Text that appears nowhere cannot be
/// anchored, and text that appears repeatedly does not say which occurrence was
/// meant; both are reported with the count so a caller can extend the anchor.
/// Multi-line anchors are matched indentation-insensitively by normalizing common
/// leading indentation in both the anchor and the canonical text.
fn anchor_range(canonical: &str, anchor: &str) -> Result<(usize, usize), Box<Diagnostic>> {
    if !anchor.contains('\n') {
        // Single-line anchors use literal matching.
        let occurrences = canonical.matches(anchor).count();
        match occurrences {
            0 => Err(Box::new(coded(
                DiagnosticCode::BldAnchorTextNotFound,
                format!(
                    "`{}` does not occur in this function",
                    sanitize_for_terminal(anchor)
                ),
                "the anchor is matched against canonical source, where comments and original spacing are normalized away",
            ))),
            1 => {
                let start = canonical.find(anchor).unwrap_or_default();
                Ok((start, start + anchor.len()))
            }
            count => Err(Box::new(coded(
                DiagnosticCode::BldAnchorTextNotUnique,
                format!(
                    "`{}` occurs {} times in this function",
                    sanitize_for_terminal(anchor),
                    count
                ),
                "extend the anchor until it matches one site only",
            ))),
        }
    } else {
        // Multi-line anchors are matched indentation-insensitively.
        let matches = anchor_sites(canonical, anchor);

        match matches.len() {
            0 => Err(Box::new(coded(
                DiagnosticCode::BldAnchorTextNotFound,
                format!(
                    "`{}` does not occur in this function",
                    sanitize_for_terminal(anchor)
                ),
                "the anchor is matched against canonical source, where comments and original spacing are normalized away",
            ))),
            1 => Ok(matches[0]),
            count => Err(Box::new(coded(
                DiagnosticCode::BldAnchorTextNotUnique,
                format!(
                    "`{}` occurs {} times in this function",
                    sanitize_for_terminal(anchor),
                    count
                ),
                "extend the anchor until it matches one site only",
            ))),
        }
    }
}

/// Check that the edit reached the function it named and nothing else.
///
/// An anchor is matched in one function but replaced in a whole file, so the
/// question worth asking afterwards is whether any other declaration moved. If
/// one did, the correspondence that placed the edit was wrong, and the answer
/// is to refuse rather than to hand back a file with a change nobody asked for.
fn confirm_only_target_changed(
    after: &str,
    target: &Statement,
    parsed_before: &Program,
) -> Result<(), Box<Diagnostic>> {
    let reparsed = parse(after)?;
    let target_index = enclosing_index(parsed_before, target);

    if reparsed.body.len() != parsed_before.body.len() {
        return Err(edit_escaped());
    }
    for (index, (old, new)) in parsed_before.body.iter().zip(&reparsed.body).enumerate() {
        if Some(index) == target_index {
            continue;
        }
        if formatter::declaration(old).text != formatter::declaration(new).text {
            return Err(edit_escaped());
        }
    }

    Ok(())
}

/// Which top-level declaration holds `target`, by identity rather than by name.
fn enclosing_index(program: &Program, target: &Statement) -> Option<usize> {
    program.body.iter().position(|entry| {
        std::ptr::eq(entry, target)
            || resolve::children(entry)
                .into_iter()
                .any(|member| std::ptr::eq(member, target))
    })
}

/// Indent a replacement body to sit where the one it replaces sat.
///
/// The range being replaced starts at the body's first token, so the
/// indentation of that first line is already in the file and the replacement
/// must not repeat it. Every line after the first carries it.
fn reindented(source: &str, body_start: usize, body: &str) -> String {
    let line_start = source
        .get(..body_start)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |index| index + 1);
    let indent: String = source
        .get(line_start..body_start)
        .unwrap_or_default()
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();

    let flattened = without_indentation(body.trim_end_matches('\n'));
    flattened
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 || line.trim().is_empty() {
                line.to_string()
            } else {
                format!("{}{}", indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Find the common prefix and suffix line counts between two texts.
///
/// Returns (prefix_count, suffix_count).
fn common_prefix_suffix(before: &str, after: &str) -> (usize, usize) {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();

    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len() - prefix)
        .min(new.len() - prefix);

    (prefix, suffix)
}

/// Render the change as one unified-diff hunk.
///
/// The hunk spans from the first differing line to the last, so a batch that
/// touches several places is reported as one stretch rather than as separate
/// hunks. That is more context than the smallest possible diff, and it is
/// always a truthful account of what the file becomes.
fn unified_diff(path: &Path, before: &str, after: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let (prefix, suffix) = common_prefix_suffix(before, after);

    let old_changed = &old[prefix..old.len() - suffix];
    let new_changed = &new[prefix..new.len() - suffix];

    let mut diff = format!(
        "--- a/{}\n+++ b/{}\n@@ -{},{} +{},{} @@\n",
        path.display(),
        path.display(),
        prefix + 1,
        old_changed.len(),
        prefix + 1,
        new_changed.len()
    );
    for line in old_changed {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new_changed {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

/// Read text the caller passed by file, or from standard input for `-`.
pub fn text_from_file(source: &str) -> Result<String, Box<Diagnostic>> {
    if source == "-" {
        let mut text = String::new();
        return match std::io::stdin().read_to_string(&mut text) {
            Ok(_) => Ok(text),
            Err(error) => Err(Box::new(coded(
                DiagnosticCode::BldInputNotReadable,
                format!("could not read standard input: {}", error),
                "write the text to standard input, or name a file instead of `-`",
            ))),
        };
    }
    fs::read_to_string(source).map_err(|error| {
        Box::new(coded(
            DiagnosticCode::BldInputNotReadable,
            format!(
                "could not read {}: {}",
                sanitize_for_terminal(source),
                error
            ),
            "check the path exists, names a file rather than a directory, and is readable",
        ))
    })
}

/// Parse exactly what was written: no normalization, no script-mode wrapping.
fn parse(source: &str) -> Result<Program, Box<Diagnostic>> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    parser
        .parse()
        .map_err(|error| Box::new(TypeError::from_syntax_error(&error).to_diagnostic()))
}

/// What checking the edited program found.
struct Checked {
    /// Whether the frontend succeeded. Warnings do not make this false.
    ok: bool,
    /// The errors the frontend reported.
    errors: Vec<Diagnostic>,
    /// The warnings the frontend reported, whether or not it succeeded.
    warnings: Vec<Diagnostic>,
}

/// Run the frontend over the edited text without putting it on disk.
fn check_source(path: &Path, source: &str) -> Checked {
    let mut pipeline = Pipeline::new();
    // Canonicalize first so that a bare filename resolves to an absolute path
    // whose parent is the working directory, not an empty path.
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(directory) = absolute.parent() {
        pipeline = pipeline.with_source_dir(directory.to_path_buf());
    }
    pipeline = pipeline.with_source_path(absolute.display().to_string());

    let outcome = pipeline.frontend(source);
    match outcome {
        Ok(result) => Checked {
            ok: true,
            errors: Vec::new(),
            warnings: result.type_checker.warnings().to_vec(),
        },
        Err(error) => {
            // A failed check reports its warnings alongside its errors, so the
            // two are told apart here rather than lost.
            let (warnings, errors) = error
                .to_diagnostics()
                .into_iter()
                .partition(|diagnostic| diagnostic.severity == Severity::Warning);
            Checked {
                ok: false,
                errors,
                warnings,
            }
        }
    }
}

/// Replace a file without ever leaving it half-written.
///
/// The new text is written beside the target and then renamed over it, which is
/// one step on POSIX. On Windows the rename fails instead if another process
/// holds the file open, which leaves the original in place — the same outcome
/// this function exists to guarantee.
fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".miri-patch")
        .tempfile_in(directory)?;
    std::io::Write::write_all(&mut temporary, contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// The hash of a file's current contents, as a caller would compute it.
fn sha256(contents: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Report a file that has moved on since the caller last read it.
fn stale_hash(source: &str, expected: &str) -> Option<Diagnostic> {
    let actual = sha256(source);
    if actual == expected {
        return None;
    }
    Some(coded(
        DiagnosticCode::BldStaleHashMismatch,
        format!(
            "the file now hashes to {}, and the edit was prepared against {}",
            actual,
            sanitize_for_terminal(expected)
        ),
        "read the file again, rebuild the edit against what it says now, and retry",
    ))
}

/// Report an edited program that does not check.
fn rejected_edit() -> Diagnostic {
    coded(
        DiagnosticCode::BldEditRejected,
        "the edited program does not check, so the file was left as it was".to_string(),
        "the errors below describe the edited program; correct the replacement text and retry",
    )
}

/// Report a name that resolves to something other than a function.
fn not_a_function(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldFunctionNotFound,
        format!(
            "`{}` does not name a function with a body",
            sanitize_for_terminal(name)
        ),
        "an edit applies to a declared function; run `miri view --outline` to list what the file declares",
    ))
}

/// Report a function whose body could not be located.
fn headerless_body(name: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        format!(
            "`{}` has no body to replace",
            sanitize_for_terminal(name)
        ),
        "a body can be replaced only on a function that declares one; an abstract declaration has none",
    ))
}

/// Report an anchor that names no whole token.
fn anchor_covers_no_token(anchor: &str) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        format!(
            "`{}` covers no complete token, so it names no bytes to replace",
            sanitize_for_terminal(anchor)
        ),
        "anchor on whole tokens rather than on part of one",
    ))
}

/// Report an edit that changed a declaration it did not name.
fn edit_escaped() -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        "the edit would have changed a declaration it did not name".to_string(),
        "this file could not be anchored reliably; rewrite the target function in canonical form and retry",
    ))
}

/// Report a byte range that does not fall on character boundaries.
fn split_failed() -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldSourceNotAnchorable,
        "the replacement range does not fall on character boundaries".to_string(),
        "anchor on whole tokens rather than on part of one",
    ))
}

/// Build a diagnostic carrying a registry code.
fn coded(code: DiagnosticCode, message: String, help: &str) -> Diagnostic {
    DiagnosticBuilder::error(code.title().to_string())
        .code(code.as_str())
        .message(message)
        .help(help.to_string())
        .build()
}

/// Build the report for a request that changed nothing.
fn refusal(
    diagnostics: Vec<Diagnostic>,
    source: String,
    source_path: Option<String>,
    revalidations: u32,
) -> PatchReport {
    let json_diagnostics = diagnostics
        .iter()
        .map(|diagnostic| to_json(diagnostic, &source, source_path.as_deref()))
        .collect::<Vec<JsonDiagnostic>>();

    report(ReportParts {
        ok: false,
        json_diagnostics,
        edits: Vec::new(),
        revalidations,
        file_written: false,
        diff: None,
        diagnostics,
        source,
        source_path,
    })
}

/// Line status in a diagnostic fingerprint.
/// Distinguishes between diagnostics that genuinely have no line (absent),
/// and diagnostics whose line cannot be mapped to a pre-existing line (unmappable).
/// These must not compare equal, or a NEW error inside the changed run can
/// fingerprint-match a spanless baseline diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DiagnosticLineStatus {
    /// The diagnostic carries an actual line number.
    Present(usize),
    /// The diagnostic has no span (e.g., a synthetic error).
    Absent,
    /// The diagnostic sits inside the changed run and cannot be mapped.
    Unmappable,
}

/// Create a fingerprint for a diagnostic to identify whether it existed before the edit.
/// Fingerprint = (path, code, message, mapped_line).
///
/// We use a set rather than a multiset because if a pre-existing error appears in the
/// edited program's error list, it matches whether or not the edit introduced duplicates.
/// A line that maps to a changed run never has a corresponding pre-existing line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiagnosticFingerprint {
    path: Option<String>,
    code: Option<String>,
    message: String,
    line: DiagnosticLineStatus,
}

fn diagnostic_fingerprint(diag: &JsonDiagnostic, is_in_changed_run: bool) -> DiagnosticFingerprint {
    let line = match diag.line {
        Some(n) => DiagnosticLineStatus::Present(n),
        None => {
            if is_in_changed_run {
                DiagnosticLineStatus::Unmappable
            } else {
                DiagnosticLineStatus::Absent
            }
        }
    };
    DiagnosticFingerprint {
        path: diag.path.clone(),
        code: diag.code.clone(),
        message: diag.message.clone(),
        line,
    }
}

/// Build a line mapping from edited line numbers to original line numbers.
/// The mapping is based on common prefix and suffix lines between before and after.
/// Returns (prefix, suffix, before_total, after_total).
fn build_line_map(before: &str, after: &str) -> (usize, usize, usize, usize) {
    let (prefix, suffix) = common_prefix_suffix(before, after);
    let before_lines = before.lines().count();
    let after_lines = after.lines().count();
    (prefix, suffix, before_lines, after_lines)
}

/// Map a diagnostic's line number from edited to original, and report if it's in the changed run.
/// Returns (is_in_changed_run, mapped_diagnostic).
fn map_diagnostic_line_with_status(
    diag: &JsonDiagnostic,
    line_map: &(usize, usize, usize, usize),
    patched_path: Option<&str>,
) -> (bool, JsonDiagnostic) {
    let mut mapped = diag.clone();
    let mut is_in_changed_run = false;

    if diag.path.as_deref() == patched_path {
        if let Some(line) = diag.line {
            let (prefix, suffix, before_total, after_total) = line_map;
            let prefix_1indexed = prefix + 1;

            if line < prefix_1indexed {
                // Line is before the edit, maps 1:1 (already correct in mapped.line).
            } else if line > after_total - suffix {
                // Line is after the edit. Calculate the shift: before_total - after_total.
                let shift = (*before_total as isize) - (*after_total as isize);
                let adjusted_isize = (line as isize) + shift;
                if adjusted_isize > 0 {
                    mapped.line = Some(adjusted_isize as usize);
                } else {
                    // Mapped line would be invalid; mark as unmappable.
                    mapped.line = None;
                }
            } else {
                // Line is inside the changed run; it cannot map to a pre-existing line.
                mapped.line = None;
                is_in_changed_run = true;
            }
        }
    }

    (is_in_changed_run, mapped)
}

/// Apply the edits to `path` and write the result.
pub fn run(
    path: &Path,
    operations: &[Operation],
    expect_sha: Option<&str>,
    mode: Mode,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    let report = patch(path, operations, expect_sha, mode);

    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => {
            if report.envelope.ok {
                if let Some(diff) = &report.diff {
                    print!("{}", diff);
                }
                if !report.diagnostics.is_empty() {
                    eprintln!("{}", report.to_pretty(color_mode));
                }
                println!("{}", summary(&report, mode));
            } else {
                eprint!("{}", report.to_pretty(color_mode));
            }
        }
    }

    if report.envelope.ok {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    }
}

/// The closing line describing what the command did.
fn summary(report: &PatchReport, mode: Mode) -> String {
    let edits = report
        .envelope
        .patch
        .as_ref()
        .map_or(0, |patch| patch.edits.len());

    let has_preexisting = report
        .envelope
        .diagnostics
        .iter()
        .any(|d| d.preexisting == Some(true));

    match mode {
        Mode::Apply => {
            if has_preexisting {
                format!(
                    "Applied {} edit(s). The file was written; {} pre-existing error(s) remain.",
                    edits,
                    report
                        .envelope
                        .diagnostics
                        .iter()
                        .filter(|d| d.preexisting == Some(true))
                        .count()
                )
            } else {
                format!("Applied {} edit(s). The edited program checks.", edits)
            }
        }
        Mode::CheckOnly | Mode::DryRun => {
            if has_preexisting {
                format!(
                    "{} edit(s) would apply. The file would have {} pre-existing error(s). Nothing was written.",
                    edits,
                    report
                        .envelope
                        .diagnostics
                        .iter()
                        .filter(|d| d.preexisting == Some(true))
                        .count()
                )
            } else {
                format!(
                    "{} edit(s) would apply and the edited program checks. Nothing was written.",
                    edits
                )
            }
        }
    }
}

/// Report edit flags that could not be read as operations, and write the result.
pub fn report_malformed(
    path: &Path,
    diagnostic: Diagnostic,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    let source = fs::read_to_string(path).unwrap_or_default();
    let report = refusal(
        vec![diagnostic],
        source,
        Some(path.display().to_string()),
        0,
    );
    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => eprint!("{}", report.to_pretty(color_mode)),
    }
    Outcome::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_fingerprint_unmappable_not_equal_to_absent() {
        // An unmappable line and an absent one must never compare equal.
        // A diagnostic with no line (absent) must not match a diagnostic whose line
        // is in the changed run (unmappable), even if path, code, and message are identical.

        let unmappable_fp = DiagnosticFingerprint {
            path: Some("test.mi".to_string()),
            code: Some("MER_TYP_001".to_string()),
            message: "type mismatch".to_string(),
            line: DiagnosticLineStatus::Unmappable,
        };

        let absent_fp = DiagnosticFingerprint {
            path: Some("test.mi".to_string()),
            code: Some("MER_TYP_001".to_string()),
            message: "type mismatch".to_string(),
            line: DiagnosticLineStatus::Absent,
        };

        assert_ne!(
            unmappable_fp, absent_fp,
            "unmappable and absent line statuses must not be equal"
        );
    }
}
