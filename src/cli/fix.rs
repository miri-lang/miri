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
#[derive(Debug)]
enum ApplyRefusal {
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
    fn describe(&self) -> String {
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

/// Run the command against `path`.
pub fn run(path: &Path, apply: bool, yes: bool, format: Format) -> Outcome {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read {}: {}", path.display(), error);
            return Outcome::Failed;
        }
    };

    let (diagnostics, ok) = collect_diagnostics(path, &source);

    if apply {
        return apply_repairs(path, &source, &diagnostics, yes, format, ok);
    }

    report_plan(&diagnostics, &source, ok, format);
    Outcome::Succeeded
}

/// Type-check `path` and return its diagnostics plus whether it checked clean.
fn collect_diagnostics(path: &Path, source: &str) -> (Vec<JsonDiagnostic>, bool) {
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

/// Print the repairs without touching any file.
fn report_plan(diagnostics: &[JsonDiagnostic], source: &str, ok: bool, format: Format) {
    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(JsonCommand::Fix, ok, diagnostics.to_vec())
                .with_exit_code(0);
            println!("{}", serialize_envelope(&envelope));
        }
        Format::Pretty => print_plan_text(diagnostics, source),
    }
}

fn print_plan_text(diagnostics: &[JsonDiagnostic], source: &str) {
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
        println!(
            "{} [{}]",
            location,
            diagnostic.code.as_deref().unwrap_or("-")
        );
        println!("  {}", diagnostic.message);
        println!("  repair {}: {}", repair.id, repair.summary);
        for edit in &repair.edits {
            println!("    {}", describe_edit(edit, source));
        }
    }

    if repairs == 0 {
        println!("No repairs available.");
    }
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

/// Apply the repairs belonging to the file named on the command line.
///
/// A diagnostic raised inside an imported file carries that file's path, so its
/// repair edits that file rather than this one. Applying it here would rewrite a
/// file the caller never named, so those repairs are reported and skipped; the
/// caller can run the command against that file directly.
fn apply_repairs(
    target: &Path,
    planned_source: &str,
    diagnostics: &[JsonDiagnostic],
    yes: bool,
    format: Format,
    ok: bool,
) -> Outcome {
    if !yes && !io::stdin().is_terminal() {
        eprintln!("error: --apply needs --yes when there is no terminal to confirm at");
        return Outcome::Refused;
    }

    let mut edits = group_edits_by_file(diagnostics);
    for (path, skipped) in take_edits_outside(&mut edits, target) {
        eprintln!(
            "note: skipped {} repair(s) in {}; run the command on that file to apply them",
            skipped.len(),
            path
        );
    }

    if edits.is_empty() {
        if format == Format::Json {
            let envelope = DiagnosticsEnvelope::new(JsonCommand::Fix, ok, diagnostics.to_vec())
                .with_exit_code(0);
            println!("{}", serialize_envelope(&envelope));
        } else {
            println!("No repairs available.");
        }
        return Outcome::Succeeded;
    }

    // Every file is rewritten only after all of them validate, so a rejected
    // edit set leaves the whole tree as it was.
    let mut rewritten = Vec::new();
    for (path, file_edits) in &edits {
        match rewrite_file(path, file_edits, planned_source) {
            Ok(contents) => rewritten.push((PathBuf::from(path), contents)),
            Err(refusal) => {
                eprintln!("error: {}", refusal.describe());
                return Outcome::Refused;
            }
        }
    }

    for (path, contents) in rewritten {
        if let Err(refusal) = write_atomically(&path, &contents) {
            eprintln!("error: {}", refusal.describe());
            return Outcome::Failed;
        }
        println!("Applied repairs to {}", path.display());
    }

    Outcome::Succeeded
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
