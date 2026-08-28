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
//! Nothing reaches disk unless the edited program checks. Every operation is
//! applied to a copy held in memory, the copy is checked once, and only then is
//! the file replaced. An edit that would not compile leaves the file exactly as
//! it was, which is what lets a caller retry without first repairing the damage
//! of the attempt before.
//!
//! What is re-checked is the target file and the modules it imports. A file
//! that imports the patched one is not re-checked here.

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
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{to_json, Diagnostic, DiagnosticBuilder, Reportable};
use crate::error::format::format_diagnostic_with_color;
use crate::error::type_error::TypeError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::pipeline::Pipeline;

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
}

/// One edit, and the function it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// The function's name, or `Class.method` for a method.
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
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return refusal(vec![unreadable(path, &error)], String::new(), source_path),
    };

    if let Some(diagnostic) = request_refused(&source, operations, expect_sha) {
        return refusal(vec![diagnostic], source, source_path);
    }

    let (edited, applied) = match fold_operations(&source, operations) {
        Ok(folded) => folded,
        Err(diagnostic) => return refusal(vec![*diagnostic], source, source_path),
    };

    // One check answers the whole batch: the operations have already been
    // folded into a single text, and it is that text the caller is asking
    // about, not the intermediate states it passed through.
    let checked = check_source(path, &edited);
    if !checked.ok {
        let mut diagnostics = vec![rejected_edit()];
        diagnostics.extend(checked.diagnostics);
        return refusal(diagnostics, source, source_path);
    }

    let file_was_written = match store(path, &edited, mode) {
        Ok(written) => written,
        Err(diagnostic) => return refusal(vec![*diagnostic], source, source_path),
    };

    applied_report(
        Applied {
            edits: applied,
            warnings: checked.warnings,
            file_was_written,
        },
        Texts {
            before: source,
            after: edited,
            path: source_path,
        },
        matches!(mode, Mode::DryRun).then_some(path),
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

/// Build the report for a batch that applied and checked.
fn applied_report(applied: Applied, texts: Texts, diff_for: Option<&Path>) -> PatchReport {
    let warnings = applied
        .warnings
        .iter()
        .map(|diagnostic| to_json(diagnostic, &texts.after, texts.path.as_deref()))
        .collect::<Vec<JsonDiagnostic>>();
    let envelope = DiagnosticsEnvelope::new(JsonCommand::Patch, true, warnings)
        .with_exit_code(0)
        .with_patch(JsonPatch {
            edits: applied.edits,
            revalidations: 1,
            file_written: applied.file_was_written,
        });
    let diff = diff_for.map(|path| unified_diff(path, &texts.before, &texts.after));

    PatchReport {
        envelope,
        file_was_written: applied.file_was_written,
        diff,
        diagnostics: Vec::new(),
        source: texts.before,
        source_path: texts.path,
    }
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

/// Apply one operation to `source`, returning the edited text and what it did.
fn apply_one(
    source: &str,
    operation: &Operation,
) -> Result<(String, JsonPatchEdit), Box<Diagnostic>> {
    let program = parse(source)?;
    let declaration = resolve::resolve(&program, &operation.function)?;
    let StatementKind::FunctionDeclaration(data) = &declaration.node else {
        return Err(not_a_function(&operation.function));
    };

    let rendered = formatter::declaration(declaration);
    let alignment = token_align::build_alignment(source, &rendered.text, data)
        .map_err(|diverged| Box::new(diverged.to_diagnostic()))?;

    let (range, replacement) = match &operation.edit {
        Edit::Anchored { old, new } => {
            let canonical = anchor_range(&rendered.text, old)?;
            let range = alignment
                .raw_range(canonical.0, canonical.1)
                .ok_or_else(|| anchor_covers_no_token(old))?;
            (range, new.clone())
        }
        Edit::Body { text } => {
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

/// Where an anchor sits in a canonical rendering.
///
/// The anchor has to name one site. Text that appears nowhere cannot be
/// anchored, and text that appears repeatedly does not say which occurrence was
/// meant; both are reported with the count so a caller can extend the anchor.
fn anchor_range(canonical: &str, anchor: &str) -> Result<(usize, usize), Box<Diagnostic>> {
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
    let line_start = source[..body_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let indent: String = source[line_start..body_start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();

    let trimmed = body.trim_end_matches('\n');
    let common = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    trimmed
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let stripped = line.get(common..).unwrap_or("");
            if index == 0 {
                stripped.to_string()
            } else if stripped.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", indent, stripped)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len() - prefix)
        .min(new.len() - prefix);

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
    diagnostics: Vec<Diagnostic>,
    /// The warnings a successful check reported.
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
            diagnostics: Vec::new(),
            warnings: result.type_checker.warnings().to_vec(),
        },
        Err(error) => Checked {
            ok: false,
            diagnostics: error.to_diagnostics(),
            warnings: Vec::new(),
        },
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
) -> PatchReport {
    let json = diagnostics
        .iter()
        .map(|diagnostic| to_json(diagnostic, &source, source_path.as_deref()))
        .collect::<Vec<JsonDiagnostic>>();

    PatchReport {
        envelope: DiagnosticsEnvelope::new(JsonCommand::Patch, false, json)
            .with_exit_code(1)
            .with_patch(JsonPatch {
                edits: Vec::new(),
                revalidations: 0,
                file_written: false,
            }),
        file_was_written: false,
        diff: None,
        diagnostics,
        source,
        source_path,
    }
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
    match mode {
        Mode::Apply => format!("Applied {} edit(s). The edited program checks.", edits),
        Mode::CheckOnly => format!(
            "{} edit(s) would apply and the edited program checks. Nothing was written.",
            edits
        ),
        Mode::DryRun => format!(
            "{} edit(s) would apply and the edited program checks. Nothing was written.",
            edits
        ),
    }
}

/// The edit flags as they arrived, before they are read as operations.
#[derive(Debug, Default, Clone)]
pub struct Request {
    /// Functions named by `--replace-in-fn`.
    pub functions: Vec<String>,
    /// Anchors given inline.
    pub old: Vec<String>,
    /// Replacements given inline.
    pub new: Vec<String>,
    /// Files, or `-`, carrying the anchors.
    pub old_file: Vec<String>,
    /// Files, or `-`, carrying the replacements.
    pub new_file: Vec<String>,
    /// Functions named by `--replace-fn`.
    pub replace_functions: Vec<String>,
    /// Files, or `-`, carrying the replacement bodies.
    pub body_file: Vec<String>,
}

/// Read the edit flags as a sequence of operations.
///
/// Anchored edits are applied in the order they were written, and body
/// replacements after them. A batch is applied to one text and checked once, so
/// a later edit sees what an earlier one did.
pub fn operations(request: &Request) -> Result<Vec<Operation>, Box<Diagnostic>> {
    reject_multiple_standard_inputs(request)?;

    let old = one_source_of(&request.old, &request.old_file, "--old", "--old-file")?;
    let new = one_source_of(&request.new, &request.new_file, "--new", "--new-file")?;

    if request.functions.len() != old.len() || request.functions.len() != new.len() {
        return Err(malformed(format!(
            "{} function(s) named for an anchored edit, with {} anchor(s) and {} replacement(s)",
            request.functions.len(),
            old.len(),
            new.len()
        )));
    }
    if request.replace_functions.len() != request.body_file.len() {
        return Err(malformed(format!(
            "{} function(s) named for a body replacement, with {} body file(s)",
            request.replace_functions.len(),
            request.body_file.len()
        )));
    }

    let mut built = Vec::new();
    for ((function, old), new) in request.functions.iter().zip(old).zip(new) {
        built.push(Operation {
            function: function.clone(),
            edit: Edit::Anchored { old, new },
        });
    }
    for (function, body) in request.replace_functions.iter().zip(&request.body_file) {
        built.push(Operation {
            function: function.clone(),
            edit: Edit::Body {
                text: text_from_file(body)?,
            },
        });
    }
    Ok(built)
}

/// Take the texts from whichever of the two flags carried them.
///
/// One flag or the other answers for a whole call. Accepting both would leave
/// the order of a batch resting on which flag an edit happened to use.
fn one_source_of(
    inline: &[String],
    files: &[String],
    inline_flag: &str,
    file_flag: &str,
) -> Result<Vec<String>, Box<Diagnostic>> {
    if !inline.is_empty() && !files.is_empty() {
        return Err(malformed(format!(
            "{} and {} were both given; one call takes its text from one of them",
            inline_flag, file_flag
        )));
    }
    if inline.is_empty() {
        return files.iter().map(|path| text_from_file(path)).collect();
    }
    Ok(inline.to_vec())
}

/// Refuse a call that would read standard input more than once.
fn reject_multiple_standard_inputs(request: &Request) -> Result<(), Box<Diagnostic>> {
    let from_input = request
        .old_file
        .iter()
        .chain(&request.new_file)
        .chain(&request.body_file)
        .filter(|source| source.as_str() == "-")
        .count();
    if from_input > 1 {
        return Err(malformed(format!(
            "{} arguments read standard input, which can be read once",
            from_input
        )));
    }
    Ok(())
}

/// Report edit flags that do not describe a coherent edit.
fn malformed(detail: String) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldMalformedEditRequest,
        detail,
        "name one function, one anchor and one replacement per edit; repeat the three together to batch edits",
    ))
}

/// Report edit flags that could not be read as operations, and write the result.
pub fn report_malformed(
    path: &Path,
    diagnostic: Diagnostic,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    let source = fs::read_to_string(path).unwrap_or_default();
    let report = refusal(vec![diagnostic], source, Some(path.display().to_string()));
    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => eprint!("{}", report.to_pretty(color_mode)),
    }
    Outcome::Failed
}
