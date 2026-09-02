// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Gate: every `class`, `enum`, `struct`, and `trait` declaration in stdlib type definitions
//! must have a doc comment placed immediately above it.
//!
//! A doc comment is a line starting with `//`. An `@attribute` line may sit
//! between the doc comment and the declaration; the gate accepts both patterns:
//!   - `// doc comment` directly above the declaration
//!   - `// doc comment` above one or more `@attribute` lines, then the declaration
//!
//! This gate covers type declarations only. Functions, constants, `runtime`, and
//! `intrinsic` declarations carry separate documentation debt; widening the gate to
//! them is a future task, not in scope here.
//!
//! The invariant is verified by parsing the output of `miri view --outline`,
//! which is the published read surface for viewing declarations, so testing the
//! real CLI ensures the published interface works as expected.
//!
//! **Scope of verification**: The gate checks that a comment line (any line starting
//! with `//`) is present immediately above each type declaration. It does not verify
//! that the comment content actually describes the declaration it sits above. This means
//! a misplaced doc comment is caught only if it leaves the original declaration
//! undocumented; if both declarations retain some comment above them, the misplacement
//! may pass silently. The primary defense against doc content drift is manual review
//! during code change.

use std::path::PathBuf;

use crate::utils::{miri_cmd, CompilerResult};

#[test]
fn test_stdlib_doc_placement() {
    let stdlib_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let mi_files = collect_mi_files(&stdlib_root);
    assert!(
        !mi_files.is_empty(),
        "No .mi files found in {}; the gate cannot pass without coverage",
        stdlib_root.display()
    );

    let mut offending = Vec::new();
    let mut total_declarations = 0;

    for mi_file in &mi_files {
        let result = run_outline_command(mi_file);
        if !result.success {
            panic!(
                "miri view --outline failed for {}.\nstdout:\n{}\nstderr:\n{}",
                mi_file.display(),
                result.stdout,
                result.stderr
            );
        }

        if result.stdout.is_empty() {
            panic!(
                "miri view --outline produced empty output for {}; gate cannot verify",
                mi_file.display()
            );
        }

        let (undocumented, decl_count) = find_undocumented_declarations(&result.stdout, mi_file);
        offending.extend(undocumented);
        total_declarations += decl_count;
    }

    assert!(
        total_declarations >= 34,
        "Gate checked only {} type declarations; expected at least 34. \
         This suggests the declaration predicate stopped matching the outline format. \
         If `miri view --outline` output changed (e.g., added `public` modifier), \
         update `is_declaration_line()` accordingly.",
        total_declarations
    );

    if !offending.is_empty() {
        let report = offending
            .iter()
            .map(|(file, decl)| format!("  {} | {}", file.display(), decl))
            .collect::<Vec<_>>()
            .join("\n");

        panic!(
            "The following type declarations lack doc comments:\n{}\n\n\
             Run `miri view --outline <file>` to inspect each, and ensure every \
             `class`, `enum`, `struct`, or `trait` line is immediately preceded by a comment line (`//`). \
             An `@attribute` line may sit between the comment and the declaration.",
            report
        );
    }
}

/// Recursively collect all `.mi` files under a directory.
fn collect_mi_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut result = Vec::new();

    let entries = std::fs::read_dir(root)
        .unwrap_or_else(|e| panic!("Failed to read directory {}: {}", root.display(), e));

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            result.extend(collect_mi_files(&path));
        } else if path.extension().is_some_and(|e| e == "mi") {
            result.push(path);
        }
    }

    result.sort();
    result
}

/// Run `miri view --outline <file>` and return the result.
fn run_outline_command(file_path: &std::path::Path) -> CompilerResult {
    let mut cmd = miri_cmd();
    cmd.arg("view")
        .arg("--outline")
        .arg(file_path.to_string_lossy().to_string());

    let output = cmd.output().expect("miri view --outline command failed");

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap_or_default(),
        stderr: String::from_utf8(output.stderr).unwrap_or_default(),
    }
}

/// Parse the outline output and find any `class`/`enum`/`trait` declarations
/// that are not immediately preceded by a comment line.
/// Returns a tuple of (undocumented declarations, total declaration count).
fn find_undocumented_declarations(
    outline: &str,
    file_path: &std::path::Path,
) -> (Vec<(PathBuf, String)>, usize) {
    let lines: Vec<&str> = outline.lines().collect();
    let mut undocumented = Vec::new();
    let mut declaration_count = 0;

    for (i, line) in lines.iter().enumerate() {
        if is_declaration_line(line) {
            declaration_count += 1;
            // Check if there's a comment line immediately before this declaration.
            // Skip over @attribute lines when searching backward.
            if !has_preceding_doc_comment(&lines, i) {
                undocumented.push((file_path.to_path_buf(), line.to_string()));
            }
        }
    }

    (undocumented, declaration_count)
}

/// Check if a line is a type declaration (`class`, `enum`, `struct`, or `trait`).
fn is_declaration_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("class ")
        || trimmed.starts_with("enum ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("trait ")
}

/// Check if a declaration line at index `i` is preceded by a comment line.
/// Skips over @attribute lines when searching backward.
fn has_preceding_doc_comment(lines: &[&str], i: usize) -> bool {
    if i == 0 {
        return false;
    }

    let mut idx = i - 1;

    // Skip backward over @attribute lines
    loop {
        let line = lines[idx].trim();
        if line.starts_with('@') {
            if idx == 0 {
                return false;
            }
            idx -= 1;
        } else {
            break;
        }
    }

    // Check if the first non-@attribute line above is a comment
    let line_before = lines[idx].trim();
    line_before.starts_with("//")
}
