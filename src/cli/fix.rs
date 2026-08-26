// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri fix` command: report the repairs the compiler determined, and
//! optionally apply them.
//!
//! A repair reaches this module already decided — the check that raised the
//! diagnostic recorded it. Nothing here inspects a diagnostic message, so this
//! module cannot invent a repair the compiler did not stand behind.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::cli::{serialize_envelope, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonEdit};
use crate::diagnostics::{DiagnosticCode, RefusedRepair};
use crate::error::diagnostic::to_json;
use crate::pipeline::Pipeline;

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The plan was reported, or the edits were applied.
    Succeeded,
    /// Applying was requested but refused; nothing was written.
    Refused,
    /// The command could not run to completion.
    Failed,
}

/// Why a set of edits cannot be applied.
#[derive(Debug, Clone)]
pub enum ApplyRefusal {
    /// Two edits cover overlapping bytes, so their combination is undefined.
    OverlappingEdits { path: String },
    /// An edit names a range the file does not have.
    RangeOutsideFile { path: String },
    /// The file could not be read or written.
    Io { path: String, error: String },
    /// The file changed after the repairs were planned against it.
    FileChanged { path: String },
}

impl ApplyRefusal {
    /// One line naming the file and what stopped it being written.
    pub fn describe(&self) -> String {
        match self {
            Self::OverlappingEdits { path } => {
                format!("repairs for {} overlap; none were applied", path)
            }
            Self::RangeOutsideFile { path } => {
                format!("a repair for {} names a range outside the file", path)
            }
            Self::Io { path, error } => format!("could not write {}: {}", path, error),
            Self::FileChanged { path } => format!(
                "{} changed after the repairs were planned; nothing was written",
                path
            ),
        }
    }
}

/// Why an apply stopped part-way.
///
/// The two are kept apart because they leave the tree in different states, and
/// the caller reports them differently.
#[derive(Debug, Clone)]
pub enum ApplyFailure {
    /// The edits could not be turned into new file contents. Nothing was
    /// written, so the tree is exactly as it was.
    Validation(ApplyRefusal),
    /// A file could not be written. Files rewritten earlier in the same run are
    /// already on disk.
    Write(ApplyRefusal),
}

/// What an apply did, as data rather than as text on a stream.
pub struct ApplyReport {
    /// Repairs withheld because their safety could not be accepted.
    pub refused: Vec<RefusedRepair>,
    /// Edits belonging to files this run was never going to write, by path.
    pub skipped: BTreeMap<String, Vec<JsonEdit>>,
    /// The files this run rewrote.
    pub applied: Vec<PathBuf>,
    /// Why the apply stopped, when it did.
    pub failure: Option<ApplyFailure>,
}

impl ApplyReport {
    /// Whether every repair this run owned was written.
    pub fn ok(&self) -> bool {
        self.refused.is_empty() && self.failure.is_none()
    }
}

/// Run the command against `path`.
pub fn run(path: &Path, apply: bool, yes: bool, allow_risky: bool, format: Format) -> Outcome {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read {}: {}", path.display(), error);
            return Outcome::Failed;
        }
    };

    let (diagnostics, ok) = diagnose(path, &source);

    if apply {
        return apply_repairs(path, &source, &diagnostics, yes, allow_risky, format, ok);
    }

    report_plan(&diagnostics, &source, ok, format);
    Outcome::Succeeded
}

/// Type-check `path` and return its diagnostics plus whether it checked clean.
///
/// The repairs are already attached to the diagnostics that carry them: a check
/// records a repair where it raises the diagnostic, so nothing here reads a
/// message to decide what to edit.
pub fn diagnose(path: &Path, source: &str) -> (Vec<JsonDiagnostic>, bool) {
    let mut pipeline = Pipeline::new();
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = absolute.parent() {
        pipeline = pipeline.with_source_dir(dir.to_path_buf());
    }
    pipeline = pipeline.with_source_path(absolute.display().to_string());

    match pipeline.frontend(source) {
        Ok(result) => {
            let warnings = result
                .type_checker
                .warnings()
                .iter()
                .map(|warning| to_json(warning, source, pipeline.source_path()))
                .collect();
            (warnings, true)
        }
        Err(error) => {
            let diagnostics = error
                .to_diagnostics()
                .iter()
                .map(|diagnostic| to_json(diagnostic, source, pipeline.source_path()))
                .collect();
            (diagnostics, false)
        }
    }
}

/// Work out what applying the repairs would do, and do it.
///
/// Nothing here writes to a stream or ends the process; the outcome, including
/// every refusal, comes back as data. The command line and a request over a
/// long-lived connection therefore reach the same decisions through the same
/// code.
///
/// A repair for a file this run was never going to write is set aside rather
/// than judged, so it cannot withhold the repairs this run owns. One refused
/// repair withholds all the rest instead of applying the safe subset: this
/// command already validates a file's edits together and writes only if every
/// one is sound, so a partial application would be the single outcome the
/// caller cannot undo by re-running.
pub fn apply(
    target: &Path,
    planned_source: &str,
    diagnostics: &[JsonDiagnostic],
    allow_risky: bool,
) -> ApplyReport {
    let mut edits = group_edits_by_file(diagnostics);
    let skipped = take_edits_outside(&mut edits, target);

    let refused = judge_repairs_to_be_written(diagnostics, &edits, allow_risky);
    if !refused.is_empty() {
        return ApplyReport {
            refused,
            skipped,
            applied: Vec::new(),
            failure: None,
        };
    }

    // Every file is rewritten only after all of them validate, so a rejected
    // edit set leaves the whole tree as it was.
    let mut rewritten = Vec::new();
    for (path, file_edits) in &edits {
        match rewrite_file(path, file_edits, planned_source) {
            Ok(contents) => rewritten.push((PathBuf::from(path), contents)),
            Err(refusal) => {
                return ApplyReport {
                    refused: Vec::new(),
                    skipped,
                    applied: Vec::new(),
                    failure: Some(ApplyFailure::Validation(refusal)),
                }
            }
        }
    }

    let mut applied = Vec::new();
    for (path, contents) in rewritten {
        if let Err(refusal) = write_atomically(&path, &contents) {
            return ApplyReport {
                refused: Vec::new(),
                skipped,
                applied,
                failure: Some(ApplyFailure::Write(refusal)),
            };
        }
        applied.push(path);
    }

    ApplyReport {
        refused: Vec::new(),
        skipped,
        applied,
        failure: None,
    }
}

/// The envelope reporting the repairs the compiler found, edited nothing.
pub fn plan_envelope(diagnostics: &[JsonDiagnostic], ok: bool) -> DiagnosticsEnvelope {
    DiagnosticsEnvelope::new(JsonCommand::Fix, ok, diagnostics.to_vec()).with_exit_code(0)
}

/// The envelope reporting what an apply did.
///
/// `ok` says whether the apply succeeded, not whether the file now compiles.
/// The two are different questions and a caller needs both: the diagnostics
/// that prompted the repairs travel in the same envelope, and a following
/// `check` says what the file looks like now. Reporting the pre-apply verdict
/// here would say `false` about an apply that did exactly what was asked.
///
/// A refusal is reported as one more diagnostic carrying the code that names
/// it, so a consumer reads it the way it reads every other diagnostic instead
/// of parsing text written for a person.
pub fn apply_envelope(report: &ApplyReport, diagnostics: &[JsonDiagnostic]) -> DiagnosticsEnvelope {
    if report.ok() {
        return DiagnosticsEnvelope::new(JsonCommand::Fix, true, diagnostics.to_vec())
            .with_exit_code(0);
    }

    let mut reported = diagnostics.to_vec();
    if !report.refused.is_empty() {
        reported.push(refusal_diagnostic());
    }
    if let Some(failure) = &report.failure {
        reported.push(failure_diagnostic(failure));
    }
    DiagnosticsEnvelope::new(JsonCommand::Fix, false, reported).with_exit_code(1)
}

/// The diagnostic that stands for a withheld set of repairs.
fn refusal_diagnostic() -> JsonDiagnostic {
    let refusal = DiagnosticCode::BldRefusedRepairs;
    JsonDiagnostic {
        severity: refusal.severity().as_str().to_string(),
        code: Some(refusal.to_string()),
        message: refusal.title().to_string(),
        path: None,
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: Some("Pass --allow-risky to apply these repairs anyway.".to_string()),
        fix_safety: Some(refusal.fix_safety().as_str().to_string()),
        repair: None,
        related: vec![],
    }
}

/// The diagnostic describing why an apply stopped part-way.
fn failure_diagnostic(failure: &ApplyFailure) -> JsonDiagnostic {
    let (refusal, help) = match failure {
        ApplyFailure::Validation(refusal) => {
            (refusal, "Nothing was written; the files are as they were.")
        }
        ApplyFailure::Write(refusal) => (
            refusal,
            "A file could not be written; files rewritten earlier in this run are already on disk.",
        ),
    };
    let code = DiagnosticCode::BldRefusedRepairs;
    JsonDiagnostic {
        severity: code.severity().as_str().to_string(),
        code: Some(code.to_string()),
        message: refusal.describe(),
        path: None,
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: Some(help.to_string()),
        fix_safety: Some(code.fix_safety().as_str().to_string()),
        repair: None,
        related: vec![],
    }
}

/// Render the repairs without touching any file. Returns a string (JSON or pretty text).
fn render_plan(diagnostics: &[JsonDiagnostic], source: &str, ok: bool, format: Format) -> String {
    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(JsonCommand::Fix, ok, diagnostics.to_vec())
                .with_exit_code(0);
            serialize_envelope(&envelope)
        }
        Format::Pretty => render_plan_text(diagnostics, source),
    }
}

/// Print the repairs without touching any file.
fn report_plan(diagnostics: &[JsonDiagnostic], source: &str, ok: bool, format: Format) {
    let output = render_plan(diagnostics, source, ok, format);
    println!("{}", output);
}

fn render_plan_text(diagnostics: &[JsonDiagnostic], source: &str) -> String {
    let mut out = String::new();
    let mut repairs = 0;
    for diagnostic in diagnostics {
        let Some(repair) = &diagnostic.repair else {
            continue;
        };
        repairs += 1;
        let location = match (&diagnostic.path, diagnostic.line, diagnostic.column) {
            (Some(path), Some(line), Some(column)) => format!("{}:{}:{}", path, line, column),
            _ => "<unknown location>".to_string(),
        };
        out.push_str(&format!(
            "{} [{}]\n",
            location,
            diagnostic.code.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!("  {}\n", diagnostic.message));
        out.push_str(&format!("  repair {}: {}\n", repair.id, repair.summary));
        for edit in &repair.edits {
            out.push_str(&format!("    {}\n", describe_edit(edit, source)));
        }
    }

    if repairs == 0 {
        out.push_str("No repairs available.\n");
    }
    out
}

/// One line describing what an edit does to the text it covers.
fn describe_edit(edit: &JsonEdit, source: &str) -> String {
    let existing = source.get(edit.start..edit.end).unwrap_or_default();
    match (existing.is_empty(), edit.replacement.is_empty()) {
        (true, _) => format!("insert `{}` at {}", edit.replacement.trim_end(), edit.start),
        (false, true) => format!("delete `{}` at {}", existing.trim(), edit.start),
        (false, false) => format!(
            "replace `{}` with `{}` at {}",
            existing.trim(),
            edit.replacement,
            edit.start
        ),
    }
}

/// Apply the repairs belonging to the file named on the command line, and say
/// what happened.
///
/// The decisions live in [`apply`]; this adds only the terminal confirmation
/// the command line owes its user, and the writing.
fn apply_repairs(
    target: &Path,
    planned_source: &str,
    diagnostics: &[JsonDiagnostic],
    yes: bool,
    allow_risky: bool,
    format: Format,
    ok: bool,
) -> Outcome {
    // A terminal to confirm at is a property of this transport, not of the
    // repairs, so the check belongs here rather than in the shared core.
    if !yes && !io::stdin().is_terminal() {
        eprintln!("error: --apply needs --yes when there is no terminal to confirm at");
        return Outcome::Refused;
    }

    let report = apply(target, planned_source, diagnostics, allow_risky);

    report_skipped_files(&report.skipped);

    if !report.refused.is_empty() {
        report_refusal(&report.refused, diagnostics, format);
        return Outcome::Refused;
    }

    match report.failure {
        Some(ApplyFailure::Validation(refusal)) => {
            eprintln!("error: {}", refusal.describe());
            return Outcome::Refused;
        }
        Some(ApplyFailure::Write(refusal)) => {
            eprintln!("error: {}", refusal.describe());
            return Outcome::Failed;
        }
        None => {}
    }

    if report.applied.is_empty() {
        if format == Format::Json {
            let envelope = DiagnosticsEnvelope::new(JsonCommand::Fix, ok, diagnostics.to_vec())
                .with_exit_code(0);
            println!("{}", serialize_envelope(&envelope));
        } else {
            println!("No repairs available.");
        }
        return Outcome::Succeeded;
    }

    for path in &report.applied {
        println!("Applied repairs to {}", path.display());
    }

    Outcome::Succeeded
}

/// Note the files whose repairs this run leaves alone.
fn report_skipped_files(skipped: &BTreeMap<String, Vec<JsonEdit>>) {
    for (path, edits) in skipped {
        eprintln!(
            "note: skipped {} repair(s) in {}; run the command on that file to apply them",
            edits.len(),
            path
        );
    }
}

/// Judge the safety of only those repairs this run would actually write.
///
/// A repair for a diagnostic raised inside an imported file was already dropped,
/// so letting it refuse would withhold edits over a change this run was never
/// going to make.
///
/// One refused repair withholds all of them rather than applying the safe
/// subset. That matches how this command already treats a file: edits are
/// validated together and written only if every one of them is sound, so a
/// partial application would be the single outcome the caller cannot undo by
/// simply re-running.
fn judge_repairs_to_be_written(
    diagnostics: &[JsonDiagnostic],
    retained: &BTreeMap<String, Vec<JsonEdit>>,
    allow_risky: bool,
) -> Vec<RefusedRepair> {
    crate::diagnostics::compute_refused_repairs(
        diagnostics
            .iter()
            .filter(|diagnostic| edits_land_in_target(diagnostic, retained)),
        allow_risky,
    )
}

/// Report repairs that were withheld, naming each one and why.
///
/// In JSON the refusal joins the diagnostics as one more entry, carrying the
/// code that names it, so a consumer reads it the same way it reads every other
/// diagnostic rather than parsing the text written for a human.
fn report_refusal(refused: &[RefusedRepair], diagnostics: &[JsonDiagnostic], format: Format) {
    for repair in refused {
        eprintln!(
            "error: [{}] repair classified as {} ({})",
            repair.code,
            repair.fix_safety,
            repair.code.title()
        );
    }

    let refusal = DiagnosticCode::BldRefusedRepairs;
    eprintln!(
        "error[{}]: {} (pass --allow-risky to override)",
        refusal,
        refusal.title()
    );

    if format != Format::Json {
        return;
    }

    let mut reported = diagnostics.to_vec();
    reported.push(refusal_diagnostic());

    let envelope = DiagnosticsEnvelope::new(JsonCommand::Fix, false, reported).with_exit_code(1);
    println!("{}", serialize_envelope(&envelope));
}

/// Whether this diagnostic's repair edits a file this run is going to write.
///
/// A diagnostic carrying no repair edits nothing, so it is not judged for
/// safety at all.
fn edits_land_in_target(
    diagnostic: &JsonDiagnostic,
    retained: &BTreeMap<String, Vec<JsonEdit>>,
) -> bool {
    diagnostic.repair.as_ref().is_some_and(|repair| {
        repair
            .edits
            .iter()
            .any(|edit| retained.contains_key(&edit.path))
    })
}

/// Collect every repair edit, keyed by the file it edits.
///
/// The map is ordered by path so a run over several files does the same work in
/// the same order every time.
fn group_edits_by_file(diagnostics: &[JsonDiagnostic]) -> BTreeMap<String, Vec<JsonEdit>> {
    let mut grouped: BTreeMap<String, Vec<JsonEdit>> = BTreeMap::new();
    for diagnostic in diagnostics {
        let Some(repair) = &diagnostic.repair else {
            continue;
        };
        for edit in &repair.edits {
            grouped
                .entry(edit.path.clone())
                .or_default()
                .push(edit.clone());
        }
    }
    grouped
}

/// Remove and return every group of edits that does not belong to `target`.
fn take_edits_outside(
    edits: &mut BTreeMap<String, Vec<JsonEdit>>,
    target: &Path,
) -> BTreeMap<String, Vec<JsonEdit>> {
    let canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let mut outside = BTreeMap::new();
    edits.retain(|path, group| {
        let same = Path::new(path)
            .canonicalize()
            .map(|candidate| candidate == canonical)
            .unwrap_or(false);
        if !same {
            outside.insert(path.clone(), std::mem::take(group));
        }
        same
    });
    outside
}

/// Produce the new contents of `path` with `edits` applied.
///
/// Edits are applied from the end of the file backwards so that each one is
/// made at the offset it was recorded against, rather than at an offset an
/// earlier edit has already shifted.
fn rewrite_file(
    path: &str,
    edits: &[JsonEdit],
    planned_source: &str,
) -> Result<String, ApplyRefusal> {
    let mut contents = fs::read_to_string(path).map_err(|error| ApplyRefusal::Io {
        path: path.to_string(),
        error: error.to_string(),
    })?;

    // Offsets describe the text the repairs were planned against. If the file
    // changed since, an offset can still land inside it while naming different
    // code, so the only safe answer is to refuse.
    if contents != planned_source {
        return Err(ApplyRefusal::FileChanged {
            path: path.to_string(),
        });
    }

    // The sort is stable, so two insertions at one offset keep the order the
    // diagnostics carrying them were reported in.
    let mut ordered: Vec<&JsonEdit> = edits.iter().collect();
    ordered.sort_by_key(|edit| (edit.start, edit.end));

    for pair in ordered.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(ApplyRefusal::OverlappingEdits {
                path: path.to_string(),
            });
        }
    }

    for edit in ordered.iter().rev() {
        if edit.end > contents.len()
            || edit.start > edit.end
            || !contents.is_char_boundary(edit.start)
            || !contents.is_char_boundary(edit.end)
        {
            return Err(ApplyRefusal::RangeOutsideFile {
                path: path.to_string(),
            });
        }
        contents.replace_range(edit.start..edit.end, &edit.replacement);
    }

    Ok(contents)
}

/// Write `contents` to `path` through a temporary file in the same directory.
///
/// The rename is what makes the change atomic: a reader sees either the old
/// file or the new one, never a partially written file.
fn write_atomically(path: &Path, contents: &str) -> Result<(), ApplyRefusal> {
    let describe = |error: std::io::Error| ApplyRefusal::Io {
        path: path.display().to_string(),
        error: error.to_string(),
    };

    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = directory.join(format!(".{}.miri-fix", file_name));

    let mut handle = fs::File::create(&temporary).map_err(&describe)?;
    handle.write_all(contents.as_bytes()).map_err(&describe)?;
    handle.sync_all().map_err(&describe)?;
    drop(handle);

    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        describe(error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start: usize, end: usize, replacement: &str) -> JsonEdit {
        JsonEdit {
            path: "main.mi".to_string(),
            start,
            end,
            replacement: replacement.to_string(),
        }
    }

    fn fixture(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("miri-fix-unit-{}.mi", name));
        fs::write(&path, contents).expect("could not write the fixture");
        path
    }

    #[test]
    fn test_edits_apply_at_the_offsets_they_were_recorded_against() {
        let path = fixture("ordering", "let a = 1\nlet b = 2\n");
        let edits = vec![edit(0, 3, "var"), edit(10, 13, "var")];

        let rewritten = rewrite_file(
            &path.display().to_string(),
            &edits,
            "let a = 1\nlet b = 2\n",
        )
        .expect("both edits sit inside the file");

        assert_eq!(rewritten, "var a = 1\nvar b = 2\n");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_overlapping_edits_are_refused_before_anything_is_written() {
        let contents = "let a = 1\n";
        let path = fixture("overlap", contents);
        let edits = vec![edit(0, 5, "var a"), edit(3, 8, "xxxxx")];

        let refusal = rewrite_file(&path.display().to_string(), &edits, contents);

        assert!(
            matches!(refusal, Err(ApplyRefusal::OverlappingEdits { .. })),
            "overlapping edits must be refused"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("the fixture should still exist"),
            contents,
            "a refusal must leave the file untouched"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_text_that_changed_since_planning_is_refused() {
        // The offsets describe the text they were planned against. Applying them
        // to different text of the same length would edit whatever now sits
        // there, so the mismatch is refused instead.
        let contents = "let a = 1\n";
        let path = fixture("changed-since-planning", contents);
        let edits = vec![edit(0, 3, "var")];

        let refusal = rewrite_file(&path.display().to_string(), &edits, "LET a = 1\n");

        assert!(matches!(refusal, Err(ApplyRefusal::FileChanged { .. })));
        assert_eq!(
            fs::read_to_string(&path).expect("the fixture should still exist"),
            contents,
            "a refusal must leave the file untouched"
        );
        let _ = fs::remove_file(&path);
    }

    /// A diagnostic carrying one repair that edits `path`.
    fn diagnostic_repairing(path: &str, label: &str) -> JsonDiagnostic {
        JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("MER_TYP_042".to_string()),
            message: "Cannot assign to immutable variable".to_string(),
            path: Some(path.to_string()),
            line: Some(1),
            column: Some(1),
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: Some(label.to_string()),
            repair: Some(crate::diagnostics::json::JsonRepair {
                id: "let-to-var".to_string(),
                summary: "Declare the variable with `var`.".to_string(),
                edits: vec![JsonEdit {
                    path: path.to_string(),
                    start: 0,
                    end: 3,
                    replacement: "var".to_string(),
                }],
            }),
            related: vec![],
        }
    }

    #[test]
    fn test_a_risky_repair_in_a_file_this_run_skips_does_not_withhold_the_others() {
        // A repair raised inside an imported file is dropped before anything is
        // written, so judging it would withhold edits over a change this run was
        // never going to make.
        let mut retained: BTreeMap<String, Vec<JsonEdit>> = BTreeMap::new();
        retained.insert("main.mi".to_string(), vec![edit(0, 3, "var")]);

        let imported = diagnostic_repairing("imported.mi", "api-changing");
        let target = diagnostic_repairing("main.mi", "local-edit");

        assert!(
            !edits_land_in_target(&imported, &retained),
            "a repair for a file this run skips is not judged"
        );
        assert!(
            edits_land_in_target(&target, &retained),
            "a repair for the named file is judged"
        );
    }

    #[test]
    fn test_an_edit_past_the_end_of_the_file_is_refused() {
        let path = fixture("out-of-range", "let a = 1\n");
        let edits = vec![edit(0, 900, "var")];

        let refusal = rewrite_file(&path.display().to_string(), &edits, "let a = 1\n");

        assert!(matches!(
            refusal,
            Err(ApplyRefusal::RangeOutsideFile { .. })
        ));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_writing_replaces_the_file_contents() {
        let path = fixture("atomic-write", "before\n");

        write_atomically(&path, "after\n").expect("the write should succeed");

        assert_eq!(
            fs::read_to_string(&path).expect("the file should exist"),
            "after\n"
        );
        assert!(
            !path.with_file_name(".main.mi.miri-fix").exists(),
            "the temporary file should not survive the rename"
        );
        let _ = fs::remove_file(&path);
    }
}
