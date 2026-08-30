// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri patch` — apply source edits with re-validation.

use std::fs;
use std::io::Write;
use std::path::Path;

use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};

use crate::utils::miri_cmd;

/// A simple program with a function we can patch.
const PROBE: &str = "fn add(a int, b int) int
    return a + b

fn main()
    println(\"ok\")
";

/// Write `source` to a temporary file and hand it to `body`.
fn with_source<T>(source: &str, body: impl FnOnce(&Path) -> T) -> T {
    let mut file = tempfile::Builder::new()
        .suffix(".mi")
        .tempfile()
        .expect("a temporary source file can be created");
    file.write_all(source.as_bytes())
        .expect("the fixture can be written");
    file.flush().expect("the fixture reaches disk");
    body(file.path())
}

/// Run `miri patch` and return (stdout, stderr, success).
fn patch(args: &[&str]) -> (String, String, bool) {
    let output = miri_cmd()
        .arg("patch")
        .args(args)
        .output()
        .expect("the patch command runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Parse the envelope out of a JSON run.
fn envelope(stdout: &str) -> DiagnosticsEnvelope {
    serde_json::from_str(stdout).expect("patch emits a parseable envelope")
}

/// Extract line:column from a pretty diagnostic output.
/// Looks for patterns like "--> path:LINE:COL" and returns (line, column).
fn extract_diagnostic_location(output: &str) -> Option<(usize, usize)> {
    for line in output.lines() {
        if line.contains("-->") {
            // Format: "--> path:LINE:COL" or just "LINE:COL" after a colon
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                // Get the last two parts (line and column)
                if let (Ok(line_num), Ok(col_num)) = (
                    parts[parts.len() - 2].parse::<usize>(),
                    parts[parts.len() - 1].parse::<usize>(),
                ) {
                    return Some((line_num, col_num));
                }
            }
        }
    }
    None
}

/// Find the line and column of a text pattern in the source.
/// Returns (line_number, column_number) where line_number is 1-indexed.
fn find_location_in_source(source: &str, pattern: &str) -> Option<(usize, usize)> {
    for (line_idx, line) in source.lines().enumerate() {
        if let Some(col_idx) = line.find(pattern) {
            return Some((line_idx + 1, col_idx + 1));
        }
    }
    None
}

/// Read the contents of a file.
fn read_file(path: &Path) -> String {
    fs::read_to_string(path).expect("can read file")
}

// A read followed by an anchored edit answers in one call.
#[test]
fn test_patch_replace_in_fn_basic() {
    with_source(PROBE, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(ok, "patch should succeed");
        let env = envelope(&stdout);
        assert_eq!(env.command, JsonCommand::Patch, "command should be Patch");
        assert!(env.ok, "patch should report ok=true");
        assert_eq!(
            env.diagnostics.len(),
            0,
            "no diagnostics when patch succeeds"
        );
    });
}

// An edit that does not check leaves the file as it was.
//
// The file starts clean, so the only thing that can break it is the edit. A
// fixture that was already broken would pass this test without proving
// anything about what a patch does.
#[test]
fn test_an_edit_introducing_an_error_leaves_the_file_unchanged() {
    with_source(PROBE, |path| {
        let before = read_file(path);

        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "a + b",
            "--new",
            "\"text\"",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "returning a string where an int is declared fails");

        let envelope = envelope(&stdout);
        assert!(!envelope.ok, "got: {stdout}");
        assert_eq!(
            envelope.diagnostics.first().and_then(|d| d.code.as_deref()),
            Some("MER_BLD_011"),
            "the refusal says the file was left alone: {stdout}"
        );
        assert!(
            envelope.diagnostics.len() > 1,
            "the errors of the edited program come with it: {stdout}"
        );
        assert_eq!(read_file(path), before, "the file is untouched");
    });
}

// A hash that no longer matches stops the edit before it is written.
#[test]
fn test_patch_stale_sha_fails_without_writing() {
    with_source(PROBE, |path| {
        let original_content = read_file(path);
        let wrong_sha = "0000000000000000000000000000000000000000000000000000000000000000";

        let (stdout, _stderr, _ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--expect-sha",
            wrong_sha,
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        let env = envelope(&stdout);
        assert!(!env.ok, "patch should report ok=false for stale sha");

        // File should be unchanged
        let final_content = read_file(path);
        assert_eq!(
            original_content, final_content,
            "file must not be written when sha doesn't match"
        );
    });
}

// A single edit reports the same payload a batch does.
#[test]
fn test_one_edit_reports_its_edit_and_one_check() {
    with_source(PROBE, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a * b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(ok, "patch should succeed");
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true");
        assert_eq!(env.diagnostics.len(), 0, "no diagnostics on success");
        let payload = env.patch.expect("a successful edit reports what it did");
        assert_eq!(payload.revalidations, 1, "one edit costs one check");
        assert_eq!(payload.edits.len(), 1, "the edit is reported: {stdout}");
        assert_eq!(
            payload.edits[0].replacement, "return a * b",
            "the payload names what was written: {stdout}"
        );
        assert!(payload.file_written, "the edit reaches disk");
    });
}

// Error path: --old matching zero times
#[test]
fn test_patch_old_text_not_found() {
    with_source(PROBE, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "nonexistent text",
            "--new",
            "replacement",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(!ok, "should fail when --old not found");
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false");
        assert_eq!(
            env.diagnostics.first().and_then(|d| d.code.as_deref()),
            Some("MER_BLD_006"),
            "an anchor that occurs nowhere is reported as such: {stdout}"
        );
    });
}

// Error path: --old matching multiple times
#[test]
fn test_patch_old_text_not_unique() {
    let source_with_duplicates = "fn test(x int) int
    var a = x + 1
    var b = x + 1
    return a + b
";
    with_source(source_with_duplicates, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "test",
            "--old",
            "x + 1",
            "--new",
            "x * 2",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(!ok, "should fail when --old matches multiple times");
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false");
        assert_eq!(
            env.diagnostics.first().and_then(|d| d.code.as_deref()),
            Some("MER_BLD_007"),
            "an anchor matching more than once is reported as such: {stdout}"
        );
    });
}

// Error path: unresolvable function name
#[test]
fn test_patch_function_not_found() {
    with_source(PROBE, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "nonexistent",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(!ok, "should fail when function not found");
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false");
        assert_eq!(
            env.diagnostics.first().and_then(|d| d.code.as_deref()),
            Some("MER_BLD_004"),
            "an unknown name is reported as such: {stdout}"
        );
    });
}

// Error path: --check-only does not write
#[test]
fn test_patch_check_only_does_not_write() {
    with_source(PROBE, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--check-only",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(ok, "check-only should succeed");
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true");

        // File should be unchanged
        let final_content = read_file(path);
        assert_eq!(
            original_content, final_content,
            "check-only must not write the file"
        );
    });
}

// Error path: --dry-run does not write
#[test]
fn test_patch_dry_run_does_not_write() {
    with_source(PROBE, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--dry-run",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(ok, "dry-run should succeed");
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true");

        // File should be unchanged
        let final_content = read_file(path);
        assert_eq!(
            original_content, final_content,
            "dry-run must not write the file"
        );
    });
}

// RED-1: Defect 1 — edit lands in wrong function (should edit beta, not alpha)
#[test]
fn test_patch_edit_correct_function_not_first_occurrence() {
    let source_with_duplicates = "fn alpha() int
    return 1 + 1

fn beta() int
    return 1 + 1
";
    with_source(source_with_duplicates, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "beta",
            "--old",
            "1 + 1",
            "--new",
            "2 + 2",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(ok, "patch should succeed");
        let env = envelope(&stdout);
        assert!(env.ok, "patch should report ok=true");

        // Verify the file was edited correctly
        let final_content = read_file(path);
        // alpha should still have "1 + 1"
        assert!(
            final_content.contains("fn alpha() int\n    return 1 + 1"),
            "alpha function should not be modified"
        );
        // beta should have "2 + 2"
        assert!(
            final_content.contains("fn beta() int\n    return 2 + 2"),
            "beta function should be modified"
        );
    });
}

// RED-2: Token alignment handles spacing normalization correctly
#[test]
fn test_patch_aligns_canonical_text_with_different_spacing() {
    let source_with_different_spacing = "fn gamma() int
    return 1+1
";
    with_source(source_with_different_spacing, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "gamma",
            "--old",
            "1 + 1",
            "--new",
            "9 + 9",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        // Token alignment should handle spacing normalization correctly
        assert!(ok, "patch should succeed with token alignment");
        let env = envelope(&stdout);
        assert!(env.ok, "patch should report ok=true");

        // Verify the file WAS edited correctly
        let final_content = read_file(path);
        assert!(
            final_content.contains("return 9 + 9"),
            "file should be modified with correct spacing in replacement"
        );
    });
}

// RED-3: Redundant parentheses cause alignment to fail
#[test]
fn test_patch_refuses_redundant_parens() {
    let source_with_redundant_parens = "fn delta() int
    return ((1 + 1))
";
    with_source(source_with_redundant_parens, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "delta",
            "--old",
            "1 + 1",
            "--new",
            "5 + 5",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        // Should fail because canonical form "(1 + 1)" and raw "((1 + 1))" have different tokens
        assert!(!ok, "patch should fail due to alignment divergence");
        let env = envelope(&stdout);
        assert!(!env.ok, "patch should report ok=false");

        // Verify the file was NOT edited
        let final_content = read_file(path);
        assert!(
            final_content.contains("((1 + 1))"),
            "file should not be modified when alignment fails"
        );
    });
}

// RED-4: Non-canonical float literals cause alignment to fail
#[test]
fn test_patch_refuses_non_canonical_float() {
    let source_with_non_canonical_float = "fn epsilon() int
    return 1.50
";
    with_source(source_with_non_canonical_float, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "epsilon",
            "--old",
            "1.5",
            "--new",
            "2.5",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        // Should fail because canonical "1.5" and raw "1.50" are different token text
        assert!(!ok, "patch should fail due to alignment divergence");
        let env = envelope(&stdout);
        assert!(!env.ok, "patch should report ok=false");

        // Verify the file was NOT edited
        let final_content = read_file(path);
        assert!(
            final_content.contains("1.50"),
            "file should not be modified when alignment fails"
        );
    });
}

// RED-5: Stale SHA-256 diagnostic carries the correct code
#[test]
fn test_patch_stale_sha_carries_code() {
    with_source(PROBE, |path| {
        let wrong_sha = "0000000000000000000000000000000000000000000000000000000000000000";

        let (stdout, _stderr, _ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--expect-sha",
            wrong_sha,
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false");
        assert!(!env.diagnostics.is_empty(), "should have diagnostics");

        let code = &env.diagnostics[0].code;
        assert_eq!(
            code.as_ref().map(|s| s.as_str()),
            Some("MER_BLD_009"),
            "stale SHA should carry BldStaleHashMismatch code"
        );
    });
}

// RED-6: Alignment refusal diagnostic carries the correct code
#[test]
fn test_patch_alignment_refusal_carries_code() {
    let source_with_redundant_parens = "fn delta() int
    return ((1 + 1))
";
    with_source(source_with_redundant_parens, |path| {
        let (stdout, _stderr, _ok) = patch(&[
            "--replace-in-fn",
            "delta",
            "--old",
            "1 + 1",
            "--new",
            "5 + 5",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false");
        assert!(!env.diagnostics.is_empty(), "should have diagnostics");

        let code = &env.diagnostics[0].code;
        assert_eq!(
            code.as_ref().map(|s| s.as_str()),
            Some("MER_BLD_010"),
            "alignment refusal should carry BldSourceNotAnchorable code"
        );
    });
}

/// A program whose functions each hold one distinct line to anchor on.
const THREE: &str = "fn one() int
    return 1

fn two() int
    return 2

fn three() int
    return 3
";

#[test]
fn test_a_batch_of_three_edits_is_checked_once() {
    with_source(THREE, |path| {
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "one",
            "--old",
            "return 1",
            "--new",
            "return 11",
            "--replace-in-fn",
            "two",
            "--old",
            "return 2",
            "--new",
            "return 22",
            "--replace-in-fn",
            "three",
            "--old",
            "return 3",
            "--new",
            "return 33",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(ok, "a batch of three edits should apply: {stdout}");

        let envelope = envelope(&stdout);
        assert_eq!(envelope.command, JsonCommand::Patch);
        let patch = envelope.patch.expect("a batch reports what it applied");
        assert_eq!(
            patch.revalidations, 1,
            "three edits share one check: {stdout}"
        );
        assert_eq!(patch.edits.len(), 3, "every edit is reported: {stdout}");
        assert!(patch.file_written, "the batch reaches disk: {stdout}");

        let written = read_file(path);
        for expected in ["return 11", "return 22", "return 33"] {
            assert!(written.contains(expected), "got: {written}");
        }
    });
}

#[test]
fn test_a_batch_whose_later_edit_fails_writes_nothing() {
    with_source(THREE, |path| {
        let before = read_file(path);
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "one",
            "--old",
            "return 1",
            "--new",
            "return 11",
            "--replace-in-fn",
            "two",
            "--old",
            "this text is not there",
            "--new",
            "return 22",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "a batch with an unanchorable edit fails: {stdout}");
        assert_eq!(
            read_file(path),
            before,
            "the earlier edit of a failed batch is not written: {stderr}"
        );
    });
}

#[test]
fn test_a_dry_run_prints_a_diff_and_writes_nothing() {
    with_source(PROBE, |path| {
        let before = read_file(path);
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "a + b",
            "--new",
            "a + b + 0",
            "--dry-run",
            &path.display().to_string(),
        ]);
        assert!(ok, "a dry run of a valid edit succeeds: {stdout}");
        assert!(stdout.contains("-    return a + b"), "got: {stdout}");
        assert!(stdout.contains("+    return a + b + 0"), "got: {stdout}");
        assert_eq!(read_file(path), before, "a dry run writes nothing");
    });
}

#[test]
fn test_a_body_is_replaced_from_a_file() {
    with_source(PROBE, |path| {
        let mut body = tempfile::Builder::new()
            .tempfile()
            .expect("a body file can be created");
        body.write_all(b"return a * b\n")
            .expect("the body can be written");
        body.flush().expect("the body reaches disk");

        let (stdout, stderr, ok) = patch(&[
            "--replace-fn",
            "add",
            "--body-file",
            &body.path().display().to_string(),
            &path.display().to_string(),
        ]);
        assert!(ok, "replacing a body succeeds: {stdout}{stderr}");

        let written = read_file(path);
        assert!(written.contains("    return a * b"), "got: {written}");
        assert!(
            written.contains("fn add(a int, b int) int"),
            "the signature survives: {written}"
        );
        assert!(
            !written.contains("return a + b"),
            "the old body is gone: {written}"
        );
    });
}

#[test]
fn test_a_body_is_replaced_from_standard_input() {
    with_source(PROBE, |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-fn",
                "add",
                "--body-file",
                "-",
                &path.display().to_string(),
            ])
            .write_stdin("return a * b\n")
            .output()
            .expect("the patch command runs");
        assert!(
            output.status.success(),
            "a body read from standard input applies: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(read_file(path).contains("    return a * b"));
    });
}

#[test]
fn test_an_edit_naming_no_replacement_is_refused() {
    with_source(PROBE, |path| {
        let before = read_file(path);
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "a + b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "an edit with no replacement is refused: {stdout}");
        let envelope = envelope(&stdout);
        assert_eq!(
            envelope.diagnostics.first().and_then(|d| d.code.as_deref()),
            Some("MER_BLD_012"),
            "got: {stdout}"
        );
        assert_eq!(read_file(path), before, "a refused request writes nothing");
    });
}

#[test]
fn test_inline_and_file_carried_anchors_are_not_mixed() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "a + b",
            "--old-file",
            "somewhere.txt",
            "--new",
            "a * b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "mixing the two anchor sources is refused: {stdout}");
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_012"),
            "got: {stdout}"
        );
    });
}

#[test]
fn test_an_ambiguous_method_name_is_refused() {
    let source = "class Point
    fn shift(d int) int
        return d

class Line
    fn shift(d int) int
        return d
";
    with_source(source, |path| {
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "shift",
            "--old",
            "return d",
            "--new",
            "return d + 1",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "an ambiguous name is refused: {stdout}");
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_005"),
            "got: {stdout}"
        );
    });
}

#[test]
fn test_comments_and_spacing_outside_the_anchor_survive() {
    let source = "// A leading note about the file.
fn keep(a int) int
    // a note inside the body
    let doubled = a + a

    return doubled
";
    with_source(source, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "keep",
            "--old",
            "a + a",
            "--new",
            "a * 2",
            &path.display().to_string(),
        ]);
        assert!(ok, "the edit applies: {stdout}{stderr}");
        assert_eq!(
            read_file(path),
            "// A leading note about the file.
fn keep(a int) int
    // a note inside the body
    let doubled = a * 2

    return doubled
",
            "only the anchored bytes change"
        );
    });
}

#[test]
fn test_a_method_is_patched_by_its_container() {
    let source = "class Point
    public x int

    fn shift(d int) int
        return self.x + d
";
    with_source(source, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "Point.shift",
            "--old",
            "self.x + d",
            "--new",
            "self.x - d",
            &path.display().to_string(),
        ]);
        assert!(ok, "a method is reached by its container: {stdout}{stderr}");
        assert!(read_file(path).contains("return self.x - d"));
    });
}

#[test]
fn test_an_unreadable_text_file_is_reported() {
    with_source(PROBE, |path| {
        let before = read_file(path);
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old-file",
            "a-file-that-is-not-there.txt",
            "--new",
            "a * b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "an unreadable anchor file is refused: {stdout}");
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_008"),
            "got: {stdout}"
        );
        assert_eq!(read_file(path), before, "nothing is written");
    });
}

#[test]
fn test_two_arguments_cannot_both_read_standard_input() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = patch(&[
            "--replace-fn",
            "add",
            "--body-file",
            "-",
            "--replace-fn",
            "main",
            "--body-file",
            "-",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "two readers of one stream is refused: {stdout}");
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_012"),
            "got: {stdout}"
        );
    });
}

#[test]
fn test_an_inline_body_is_replaced_in_place() {
    with_source("fn twice(n int) int: n * 2\n", |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-fn",
                "twice",
                "--body-file",
                "-",
                &path.display().to_string(),
            ])
            .write_stdin("n * 3\n")
            .output()
            .expect("the patch command runs");
        assert!(
            output.status.success(),
            "a colon body is replaced: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            read_file(path),
            "fn twice(n int) int: n * 3\n",
            "the body stays inline"
        );
    });
}

#[test]
fn test_a_method_body_is_replaced_at_its_own_indentation() {
    let source = "class Point
    public x int

    fn shift(d int) int
        return self.x + d
";
    with_source(source, |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-fn",
                "Point.shift",
                "--body-file",
                "-",
                &path.display().to_string(),
            ])
            .write_stdin("let start = self.x\nreturn start - d\n")
            .output()
            .expect("the patch command runs");
        assert!(
            output.status.success(),
            "a method body is replaced: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            read_file(path),
            "class Point
    public x int

    fn shift(d int) int
        let start = self.x
        return start - d
",
            "every line of the new body sits at the body's indentation"
        );
    });
}

#[test]
fn test_carriage_returns_survive_an_edit() {
    with_source("fn r() int\r\n    return 1\r\n", |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "r",
            "--old",
            "return 1",
            "--new",
            "return 7",
            &path.display().to_string(),
        ]);
        assert!(
            ok,
            "a file with carriage returns is edited: {stdout}{stderr}"
        );
        assert_eq!(
            read_file(path),
            "fn r() int\r\n    return 7\r\n",
            "the line endings the author used are left alone"
        );
    });
}

#[test]
fn test_a_later_edit_sees_what_an_earlier_one_wrote() {
    with_source(THREE, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "one",
            "--old",
            "return 1",
            "--new",
            "return 41",
            "--replace-in-fn",
            "one",
            "--old",
            "return 41",
            "--new",
            "return 42",
            &path.display().to_string(),
        ]);
        assert!(ok, "the second edit anchors on the first: {stdout}{stderr}");
        assert!(
            read_file(path).contains("return 42"),
            "got: {}",
            read_file(path)
        );
    });
}

#[test]
fn test_an_anchor_covering_part_of_a_token_is_refused() {
    with_source("fn p() int\n    return 1234\n", |path| {
        let before = read_file(path);
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "p",
            "--old",
            "123",
            "--new",
            "99",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(!ok, "half of a literal names no bytes to replace: {stdout}");
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_010"),
            "got: {stdout}"
        );
        assert_eq!(read_file(path), before, "nothing is written");
    });
}

#[test]
fn test_an_anchor_found_only_in_a_comment_is_refused() {
    let source = "fn c() int
    // return 99 is a note, not code
    return 1
";
    with_source(source, |path| {
        let before = read_file(path);
        let (stdout, _, ok) = patch(&[
            "--replace-in-fn",
            "c",
            "--old",
            "return 99",
            "--new",
            "return 5",
            "--format",
            "json",
            &path.display().to_string(),
        ]);
        assert!(
            !ok,
            "comments are absent from the text an anchor matches: {stdout}"
        );
        assert_eq!(
            envelope(&stdout)
                .diagnostics
                .first()
                .and_then(|d| d.code.as_deref()),
            Some("MER_BLD_006"),
            "got: {stdout}"
        );
        assert_eq!(read_file(path), before, "nothing is written");
    });
}

// An edit that leaves a pre-existing error untouched is written.
// is written, and the envelope carries that error marked preexisting: true
#[test]
fn test_patch_accepts_edit_leaving_preexisting_error_untouched() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "patch should succeed despite pre-existing error: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");

        let final_content = read_file(path);
        assert_ne!(final_content, original_content, "file should be modified");
        assert!(
            final_content.contains("return a - b"),
            "edit should be applied: {final_content}"
        );

        // Should report pre-existing error as such
        let error_diag = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("pre-existing type error should be reported");
        assert_eq!(
            error_diag.preexisting,
            Some(true),
            "pre-existing error should be marked: {stdout}"
        );
    });
}

// An edit that adds a new diagnostic is refused.
// with MER_BLD_011 and writes nothing
#[test]
fn test_patch_rejects_edit_that_introduces_new_error() {
    let source_with_existing_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_existing_error, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return \"text\"",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            !ok,
            "patch should fail when introducing new error: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false: {stdout}");

        let final_content = read_file(path);
        assert_eq!(
            final_content, original_content,
            "file should not be modified when edit introduces new error"
        );

        // Should have MER_BLD_011 refusal
        let refusal = env
            .diagnostics
            .iter()
            .find(|d| d.code.as_deref() == Some("MER_BLD_011"))
            .expect("MER_BLD_011 should be reported: {stdout}");
        assert_eq!(
            refusal.preexisting, None,
            "refusal itself should not be marked"
        );
    });
}

// An edit that removes one of two errors is written and the other reported.
// is written and the remaining one is reported
#[test]
fn test_patch_accepts_edit_that_removes_one_error() {
    let source_with_two_errors = "fn bad1() int
    return \"wrong\"

fn bad2() int
    return \"also wrong\"

fn main()
    println(\"ok\")
";
    with_source(source_with_two_errors, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "bad1",
            "--old",
            "return \"wrong\"",
            "--new",
            "return 42",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(ok, "patch should succeed when removing one error: {stdout}");
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");

        let final_content = read_file(path);
        assert_ne!(final_content, original_content, "file should be modified");
        assert!(
            final_content.contains("return 42"),
            "edit should be applied: {final_content}"
        );

        // Should report the remaining error as pre-existing
        let remaining_error = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("remaining error should be reported: {stdout}");
        assert_eq!(
            remaining_error.preexisting,
            Some(true),
            "remaining error should be marked as pre-existing: {stdout}"
        );
    });
}

// A check that ran and refused reports one revalidation, not zero.
// when a check actually ran and refused
#[test]
fn test_patch_revalidations_counts_actual_checks() {
    let source_with_existing_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_existing_error, |path| {
        let (stdout, _stderr, _ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return \"also wrong\"",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        let env = envelope(&stdout);
        assert!(!env.ok, "patch should fail: {stdout}");

        let patch_payload = env
            .patch
            .as_ref()
            .expect("patch should be present in refusal");
        assert_eq!(
            patch_payload.revalidations, 1,
            "revalidations should be 1 when check actually ran and refused: {stdout}"
        );
    });
}

// A parse failure precedes any check, so it reports no revalidation.
#[test]
fn test_patch_parse_error_reports_zero_revalidations() {
    let source_with_parse_error = "fn broken() int
    return \"text\"

// Intentionally unclosed block
fn main()
    println(\"ok\"
";
    with_source(source_with_parse_error, |path| {
        let (stdout, _stderr, _ok) = patch(&[
            "--replace-in-fn",
            "main",
            "--old",
            "println",
            "--new",
            "println",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        let env = envelope(&stdout);
        assert!(!env.ok, "patch should fail on parse error: {stdout}");

        let patch_payload = env.patch.as_ref().expect("patch should be present");
        assert_eq!(
            patch_payload.revalidations, 0,
            "revalidations should be 0 for parse errors (out of scope): {stdout}"
        );
    });
}

// Test gap fix: pretty mode output shows pre-existing errors correctly.
// Acceptance criterion: accepted-and-clean still prints "The edited program checks"
#[test]
fn test_patch_pretty_output_clean_edit() {
    with_source(PROBE, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            &path.display().to_string(),
        ]);

        assert!(ok, "patch should succeed");
        assert!(
            stdout.contains("The edited program checks."),
            "clean edit should say program checks: {stdout}{stderr}"
        );
        assert!(
            !stdout.contains("pre-existing"),
            "clean edit should not mention pre-existing: {stdout}{stderr}"
        );
    });
}

// Test gap fix: pretty mode output with pre-existing errors.
// Acceptance criterion: accepted-with-pre-existing prints the remaining error
// and a summary that does NOT claim the program checks
#[test]
fn test_patch_pretty_output_with_preexisting_error() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "patch should succeed despite pre-existing error: {stdout}{stderr}"
        );

        let output = format!("{}{}", stdout, stderr);
        assert!(
            output.contains("MER_TYP"),
            "output should contain type error code: {output}"
        );
        assert!(
            output.contains("String") || output.contains("return \"text\""),
            "output should show the type mismatch: {output}"
        );
        assert!(
            !output.contains("The edited program checks."),
            "output should NOT claim program checks when errors remain: {output}"
        );
        assert!(
            output.contains("pre-existing error"),
            "output should mention pre-existing errors: {output}"
        );

        // The rendered line must be where the offending text actually sits.
        let (reported_line, _) = extract_diagnostic_location(&output)
            .expect("pretty output should include error location");
        let (expected_line, _) = find_location_in_source(source_with_error, "return \"text\"")
            .expect("pattern should exist in source");
        assert_eq!(
            reported_line, expected_line,
            "error should be reported at the correct line"
        );
    });
}

// Test gap fix: refusal path prints underlying type errors in pretty mode.
// Acceptance criterion: the refusal path prints the underlying type error,
// not just the MER_BLD_011 notice
#[test]
fn test_patch_pretty_refusal_shows_errors() {
    let source_with_existing_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_existing_error, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return \"text\"",
            &path.display().to_string(),
        ]);

        assert!(!ok, "patch should fail when introducing new error");

        let output = format!("{}{}", stdout, stderr);
        assert!(
            output.contains("MER_BLD_011"),
            "output should contain edit refusal code: {output}"
        );
        assert!(
            output.contains("MER_TYP"),
            "output should contain the underlying type error code: {output}"
        );
        assert!(
            output.contains("return \"text\"") || output.contains("type error"),
            "output should describe the error, not just name the code: {output}"
        );

        // The rendered line must be the one the edited code put the error on.
        let (reported_line, _) = extract_diagnostic_location(&output)
            .expect("pretty output should include error location");
        // The new error is from the edited code "return \"text\"" in the add function
        // which is at line 2 (inside add function).
        // We're looking for where this appears in the EDITED version (as reported).
        // Since we replaced "return a + b" with "return \"text\"", the error should be
        // reported at line 2 where the replacement was made
        assert_eq!(
            reported_line, 2,
            "new error in add function should be reported at line 2"
        );
    });
}

// The accepted path reports the edited file's line numbers, not the original's.
// Edit that adds lines before a pre-existing error should report the error's line number
// in the edited file.
#[test]
fn test_patch_accepted_with_added_lines_reports_correct_line_number() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-fn",
                "add",
                "--body-file",
                "-",
                "--format",
                "json",
                &path.display().to_string(),
            ])
            .write_stdin("var t = a\nvar u = b\nreturn t + u\n")
            .output()
            .expect("the patch command runs");

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "patch should succeed with pre-existing error"
        );

        let env = envelope(&stdout);
        assert!(env.ok, "patch should report ok=true: {stdout}");

        // Read the edited file to find where broken() now is
        let edited = read_file(path);
        let broken_line = edited
            .lines()
            .position(|line| line.contains("fn broken()"))
            .map(|i| i + 1)
            .expect("broken() should exist in edited file");

        // Find the type error in the JSON
        let type_error = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("type error should be reported");

        // The error's reported line should match where it is in the edited file
        assert_eq!(
            type_error.line,
            Some(broken_line + 1),
            "reported line should match error's position in edited file. Edited file:\n{edited}"
        );
    });
}

// The accepted path maps line numbers correctly when the edit removes lines.
#[test]
fn test_patch_accepted_with_removed_lines_reports_correct_line_number() {
    let source_with_error = "fn add(a int, b int) int
    var t = a
    var u = b
    return t + u

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-fn",
                "add",
                "--body-file",
                "-",
                "--format",
                "json",
                &path.display().to_string(),
            ])
            .write_stdin("return a + b\n")
            .output()
            .expect("the patch command runs");

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "patch should succeed with pre-existing error"
        );

        let env = envelope(&stdout);
        assert!(env.ok, "patch should report ok=true: {stdout}");

        // Read the edited file to find where broken() now is
        let edited = read_file(path);
        let broken_line = edited
            .lines()
            .position(|line| line.contains("fn broken()"))
            .map(|i| i + 1)
            .expect("broken() should exist in edited file");

        // Find the type error
        let type_error = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("type error should be reported");

        // The error's reported line should match where it is in the edited file
        assert_eq!(
            type_error.line,
            Some(broken_line + 1),
            "reported line should match error's position in edited file. Edited file:\n{edited}"
        );
    });
}

// The JSON envelope and the rendered output agree on a diagnostic's position.
#[test]
fn test_patch_accepted_json_and_pretty_agree_on_line_numbers() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        // Parse JSON to get reported line
        let json_output = miri_cmd()
            .arg("patch")
            .args([
                "--replace-in-fn",
                "add",
                "--old",
                "return a + b",
                "--new",
                "return a - b",
                "--format",
                "json",
                &path.display().to_string(),
            ])
            .output()
            .expect("patch runs");
        let json_stdout = String::from_utf8_lossy(&json_output.stdout);
        let env = envelope(&json_stdout.as_ref());

        let json_error = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("type error should be in JSON");

        let json_line = json_error.line.expect("JSON error should have line number");
        let json_column = json_error
            .column
            .expect("JSON error should have column number");

        // Parse pretty output to get reported line and column (run on fresh file)
        with_source(source_with_error, |path2| {
            let pretty_output = miri_cmd()
                .arg("patch")
                .args([
                    "--replace-in-fn",
                    "add",
                    "--old",
                    "return a + b",
                    "--new",
                    "return a - b",
                    &path2.display().to_string(),
                ])
                .output()
                .expect("patch runs");
            let pretty_combined = format!(
                "{}{}",
                String::from_utf8_lossy(&pretty_output.stdout),
                String::from_utf8_lossy(&pretty_output.stderr)
            );

            // Read the position back out of the rendering rather than matching a substring.
            let (pretty_line, pretty_col) = extract_diagnostic_location(&pretty_combined)
                .expect("pretty output should include error location");
            assert_eq!(
                pretty_line, json_line,
                "pretty and JSON should report same line number. Pretty:\n{}",
                pretty_combined
            );
            assert_eq!(
                pretty_col, json_column,
                "pretty and JSON should report same column number. Pretty:\n{}",
                pretty_combined
            );
        });
    });
}

// The refusal renders the edited program's diagnostics against the edited text.
// When edit introduces a new error by changing existing code, the pretty output
// must show the edited text, not the original.
#[test]
fn test_patch_refusal_renders_against_edited_text() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return \"wrong\"",
            &path.display().to_string(),
        ]);

        assert!(!ok, "patch should fail when introducing new error");

        let output = format!("{}{}", stdout, stderr);

        // The pretty output should show the edited text "return \"wrong\"",
        // not the original "return a + b"
        assert!(
            output.contains("return \"wrong\""),
            "pretty output should show the edited text, got:\n{output}"
        );

        // Verify it doesn't show the original code that was replaced
        assert!(
            !output.contains("return a + b"),
            "pretty output should not show original unmodified code"
        );

        // The reported location must point into the edited text.
        let (reported_line, _) = extract_diagnostic_location(&output)
            .expect("pretty output should include error location");
        // The edited "return \"wrong\"" is at line 2 of the edited file
        assert_eq!(
            reported_line, 2,
            "error in edited add function should be reported at line 2"
        );
    });
}

// Two errors sharing a code and a message, one pre-existing and one introduced,
// must be marked correctly.
#[test]
fn test_patch_identical_code_and_message_split() {
    // A source with two functions, each with the same type error (string where int is expected)
    let source_with_two_identical_errors = "fn bad1() int
    return \"text\"

fn bad2() int
    return \"also string\"

fn main()
    println(\"ok\")
";
    with_source(source_with_two_identical_errors, |path| {
        // This edit will fix bad1 but keep bad2's error
        // The baseline has a MER_TYP error from bad1 ("text" at line 2)
        // The edited program will have the same MER_TYP error from bad2 (now at a different line)
        // These have the same code and message but different locations
        // The one from bad2 should be marked preexisting, not treated as a new error
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "bad1",
            "--old",
            "return \"text\"",
            "--new",
            "return 1",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "patch should succeed when remaining error is pre-existing: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "patch should report ok=true: {stdout}");

        // Check that the remaining error is marked as preexisting
        let remaining_error = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("remaining error should be reported: {stdout}");
        assert_eq!(
            remaining_error.preexisting,
            Some(true),
            "remaining error with same code/message should be marked preexisting: {stdout}"
        );
    });
}

// `--check-only` beside a pre-existing error writes nothing.
#[test]
fn test_patch_check_only_with_preexisting_error() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--check-only",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "check-only should succeed with pre-existing error: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");
        assert!(
            env.patch.as_ref().map_or(false, |p| !p.file_written),
            "file_written should be false: {stdout}"
        );

        // File should be unchanged
        let final_content = read_file(path);
        assert_eq!(
            original_content, final_content,
            "check-only must not write the file"
        );

        // Should report the pre-existing error
        let error_diag = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("pre-existing error should be reported");
        assert_eq!(
            error_diag.preexisting,
            Some(true),
            "error should be marked preexisting: {stdout}"
        );
    });
}

// `--dry-run` beside a pre-existing error writes nothing.
#[test]
fn test_patch_dry_run_with_preexisting_error() {
    let source_with_error = "fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    println(\"ok\")
";
    with_source(source_with_error, |path| {
        let original_content = read_file(path);

        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--dry-run",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "dry-run should succeed with pre-existing error: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");
        assert!(
            env.patch.as_ref().map_or(false, |p| !p.file_written),
            "file_written should be false: {stdout}"
        );

        // File should be unchanged
        let final_content = read_file(path);
        assert_eq!(
            original_content, final_content,
            "dry-run must not write the file"
        );

        // Should report the pre-existing error
        let error_diag = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("pre-existing error should be reported");
        assert_eq!(
            error_diag.preexisting,
            Some(true),
            "error should be marked preexisting: {stdout}"
        );
    });
}

// A file with no trailing newline still maps positions correctly.
#[test]
fn test_patch_no_trailing_newline() {
    let source_with_error_no_newline = "fn add(a int, b int) int\n    return a + b\n\nfn broken() int\n    return \"text\"\n\nfn main()\n    println(\"ok\")";
    with_source(source_with_error_no_newline, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "patch should succeed with file lacking trailing newline: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");

        // Should report the pre-existing error
        let error_diag = env
            .diagnostics
            .iter()
            .find(|d| {
                d.code
                    .as_deref()
                    .map(|c| c.starts_with("MER_TYP"))
                    .unwrap_or(false)
            })
            .expect("pre-existing error should be reported");
        assert_eq!(
            error_diag.preexisting,
            Some(true),
            "error should be marked preexisting: {stdout}"
        );
    });
}

// An empty file is refused by anchoring rather than crashing.
#[test]
fn test_patch_empty_source_file() {
    with_source("", |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "any",
            "--old",
            "x",
            "--new",
            "y",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(!ok, "patch should fail on empty file: {stdout}");
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false: {stdout}");
        // Should have a parse or anchor error, not a panic
        assert!(
            !env.diagnostics.is_empty(),
            "should report diagnostic(s), not panic: {stdout}"
        );
    });
}

/// A program whose check reports a deprecation warning as well as a type error.
const WARNING_AND_ERROR: &str = "@deprecated(\"use current\")
fn old() int
    return 1

fn add(a int, b int) int
    return a + b

fn broken() int
    return \"text\"

fn main()
    let value = old()
    println(\"ok\")
";

// A warning survives a failed check: an edit accepted over a pre-existing error
// still reports the warnings the same check found.
#[test]
fn test_patch_reports_warning_beside_preexisting_error() {
    with_source(WARNING_AND_ERROR, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(
            ok,
            "patch should succeed over a pre-existing error: {stdout}"
        );
        let env = envelope(&stdout);
        assert!(env.ok, "should report ok=true: {stdout}");

        let warning = env
            .diagnostics
            .iter()
            .find(|d| d.severity == "warning")
            .expect("the deprecation warning should be reported");
        assert_eq!(
            warning.code.as_deref(),
            Some("MER_TYP_027"),
            "the warning should be the deprecation one: {stdout}"
        );
        assert_eq!(
            warning.preexisting, None,
            "a warning is not partitioned against the baseline: {stdout}"
        );
        assert!(
            env.diagnostics
                .iter()
                .any(|d| d.severity == "error" && d.preexisting == Some(true)),
            "the pre-existing error should still be reported: {stdout}"
        );
    });
}

// A refusal reports the warnings of the edited program too, so the JSON payload
// says what the rendering says.
#[test]
fn test_patch_refusal_reports_warnings() {
    with_source(WARNING_AND_ERROR, |path| {
        let (stdout, _stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return nope",
            "--format",
            "json",
            &path.display().to_string(),
        ]);

        assert!(!ok, "an edit introducing an error is refused: {stdout}");
        let env = envelope(&stdout);
        assert!(!env.ok, "should report ok=false: {stdout}");

        let warning = env
            .diagnostics
            .iter()
            .find(|d| d.severity == "warning")
            .expect("the deprecation warning should be reported");
        assert_eq!(
            warning.code.as_deref(),
            Some("MER_TYP_027"),
            "the warning should be the deprecation one: {stdout}"
        );
    });
}

// Warnings reach the human rendering on every path, including the one where the
// edited program checks cleanly.
#[test]
fn test_patch_pretty_output_reports_warning_on_clean_apply() {
    let source = "@deprecated(\"use current\")
fn old() int
    return 1

fn add(a int, b int) int
    return a + b

fn main()
    let value = old()
    println(\"ok\")
";
    with_source(source, |path| {
        let (stdout, stderr, ok) = patch(&[
            "--replace-in-fn",
            "add",
            "--old",
            "return a + b",
            "--new",
            "return a - b",
            &path.display().to_string(),
        ]);

        assert!(ok, "the edit checks cleanly: {stdout}");
        assert!(
            stderr.contains("MER_TYP_027"),
            "the deprecation warning should be rendered: {stderr}"
        );

        let written = read_file(path);
        let (expected_line, _) = find_location_in_source(&written, "= old()")
            .expect("the deprecated call is in the written file");
        let (reported_line, _) =
            extract_diagnostic_location(&stderr).expect("the warning carries a position");
        assert_eq!(
            reported_line, expected_line,
            "the warning is rendered against the edited text: {stderr}"
        );
    });
}

/// Write `text` to a temporary file and hand its path to `body`.
///
/// A declaration travels by file rather than on the command line so that a
/// multi-line body needs no shell quoting.
fn with_body<T>(text: &str, body: impl FnOnce(&str) -> T) -> T {
    let mut file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile()
        .expect("a temporary body file can be created");
    file.write_all(text.as_bytes())
        .expect("the body can be written");
    file.flush().expect("the body reaches disk");
    body(&file.path().display().to_string())
}

/// The code every diagnostic in an envelope carries, for assertions.
fn codes(env: &DiagnosticsEnvelope) -> Vec<String> {
    env.diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect()
}

// A file with no `--after` gets the new declaration at its end, separated by a
// blank line and leaving the trailing newline as the author left it.
#[test]
fn test_insert_appends_a_top_level_function() {
    with_source(PROBE, |path| {
        with_body("fn answer() int\n    return 42", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "answer",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(ok, "an insert of a new name applies: {stdout}{stderr}");
            assert_eq!(
                read_file(path),
                "fn add(a int, b int) int\n    return a + b\n\nfn main()\n    println(\"ok\")\n\nfn answer() int\n    return 42\n",
                "the declaration is appended below the last one, one blank line down"
            );
        });
    });
}

// `--after` puts the declaration between the one it names and the next.
#[test]
fn test_insert_after_places_the_declaration_between_two_others() {
    let source = "fn first() int\n    return 1\n\nfn third() int\n    return 3\n\nfn main()\n    println(\"ok\")\n";
    with_source(source, |path| {
        with_body("fn second() int\n    return 2", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "second",
                "--after",
                "first",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                ok,
                "an insert after a named function applies: {stdout}{stderr}"
            );
            assert_eq!(
                read_file(path),
                "fn first() int\n    return 1\n\nfn second() int\n    return 2\n\nfn third() int\n    return 3\n\nfn main()\n    println(\"ok\")\n",
                "the new declaration sits between the one it followed and the next"
            );
        });
    });
}

// A method goes inside its container, at the depth that container's own
// members are written at — not at the end of the file.
//
// The container is one with a plain field, which is the case that cannot be
// anchored through the canonical rendering: the parser records a field as
// mutable, so the renderer writes the `var` the file does not have.
#[test]
fn test_insert_places_a_method_inside_its_container() {
    let source = "class Order\n    total int\n    quantity int\n\nfn main()\n    println(\"ok\")\n";
    with_source(source, |path| {
        with_body(
            "fn subtotal() int\n    return self.total * self.quantity",
            |body| {
                let (stdout, stderr, ok) = patch(&[
                    "--insert-fn",
                    "Order.subtotal",
                    "--body-file",
                    body,
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(ok, "a method insert applies: {stdout}{stderr}");
                assert_eq!(
                    read_file(path),
                    "class Order\n    total int\n    quantity int\n\n    fn subtotal() int\n        return self.total * self.quantity\n\nfn main()\n    println(\"ok\")\n",
                    "the method is written inside the class, at its members' depth"
                );
            },
        );
    });
}

// A method inserted after a named sibling sits at that sibling's depth.
#[test]
fn test_insert_after_a_method_stays_inside_the_container() {
    let source = "class Order\n    total int\n\n    fn first() int\n        return 1\n\n    fn last() int\n        return 9\n";
    with_source(source, |path| {
        with_body("fn middle() int\n    return 5", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "Order.middle",
                "--after",
                "Order.first",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(ok, "an insert after a method applies: {stdout}{stderr}");
            assert_eq!(
                read_file(path),
                "class Order\n    total int\n\n    fn first() int\n        return 1\n\n    fn middle() int\n        return 5\n\n    fn last() int\n        return 9\n",
                "the method lands between its two siblings, at their depth"
            );
        });
    });
}

// A name the file already declares is refused, and nothing is written.
#[test]
fn test_insert_refuses_a_name_the_file_already_declares() {
    let source = "fn helper() int\n    return 42\n\nfn main()\n    println(\"ok\")\n";
    with_source(source, |path| {
        with_body("fn helper() int\n    return 43", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "helper",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "a duplicate name is refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_017".to_string()),
                "the refusal carries MER_BLD_017: {stdout}"
            );
            assert_eq!(read_file(path), source, "the file is left as it was");
        });
    });
}

// A top-level name and a method of that name are different declarations, and a
// file may hold both.
#[test]
fn test_insert_allows_a_top_level_name_beside_a_method_of_that_name() {
    let source = "class Box\n    size int\n\n    fn total() int\n        return self.size\n";
    with_source(source, |path| {
        with_body("fn total() int\n    return 7", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "total",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                ok,
                "a top-level name does not collide with a method of that name: {stdout}{stderr}"
            );
            assert_eq!(
                read_file(path),
                "class Box\n    size int\n\n    fn total() int\n        return self.size\n\nfn total() int\n    return 7\n",
                "the top-level function is written at the top level, not into the class"
            );
        });
    });
}

// A method addressed to a container the file does not declare is refused with
// the code the agent contract names for it.
#[test]
fn test_insert_refuses_a_method_into_a_container_the_file_lacks() {
    let source = "fn main()\n    println(\"ok\")\n";
    with_source(source, |path| {
        with_body("fn method() int\n    return 42", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "Missing.method",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                !ok,
                "a method into an undeclared container is refused: {stdout}"
            );
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_004".to_string()),
                "the refusal carries MER_BLD_004: {stdout}"
            );
            assert_eq!(read_file(path), source, "the file is left as it was");
        });
    });
}

// `--after` naming nothing is refused the same way any unresolved name is.
#[test]
fn test_insert_refuses_an_after_that_names_nothing() {
    with_source(PROBE, |path| {
        with_body("fn answer() int\n    return 42", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "answer",
                "--after",
                "nowhere",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "an anchor that names nothing is refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_004".to_string()),
                "the refusal carries MER_BLD_004: {stdout}"
            );
            assert_eq!(read_file(path), PROBE, "the file is left as it was");
        });
    });
}

// Text declaring a name other than the one asked for is refused, because the
// caller would otherwise be told it inserted something it did not.
#[test]
fn test_insert_refuses_text_declaring_another_name() {
    with_source(PROBE, |path| {
        with_body("fn something_else() int\n    return 42", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "answer",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "a body declaring another name is refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_012".to_string()),
                "the refusal carries MER_BLD_012: {stdout}"
            );
            assert_eq!(read_file(path), PROBE, "the file is left as it was");
        });
    });
}

// An insert that does not check is refused, and the file keeps its own text.
#[test]
fn test_insert_that_does_not_check_leaves_the_file_alone() {
    with_source(PROBE, |path| {
        with_body("fn answer() int\n    return missing_name()", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "answer",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "an insert that does not check is refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_011".to_string()),
                "the refusal reports the edited program was rejected: {stdout}"
            );
            assert_eq!(read_file(path), PROBE, "the file is left as it was");
        });
    });
}

// The separators an insert introduces follow the endings the file already uses.
#[test]
fn test_insert_follows_the_files_line_endings() {
    with_source("fn r() int\r\n    return 1\r\n", |path| {
        with_body("fn s() int\n    return 2", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "s",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                ok,
                "a file with carriage returns is inserted into: {stdout}{stderr}"
            );
            assert_eq!(
                read_file(path),
                "fn r() int\r\n    return 1\r\n\r\nfn s() int\r\n    return 2\r\n",
                "the line endings the author used are the ones the insert writes"
            );
        });
    });
}

// A file with no trailing newline keeps having none.
#[test]
fn test_insert_into_a_file_without_a_trailing_newline() {
    with_source("fn a() int\n    return 1", |path| {
        with_body("fn b() int\n    return 2", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "b",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                ok,
                "a file with no trailing newline is inserted into: {stdout}{stderr}"
            );
            assert_eq!(
                read_file(path),
                "fn a() int\n    return 1\n\nfn b() int\n    return 2",
                "no trailing newline is invented"
            );
        });
    });
}

// An empty file takes the declaration with no blank line above it.
#[test]
fn test_insert_into_an_empty_file() {
    with_source("", |path| {
        with_body("fn main()\n    println(\"ok\")", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "main",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(ok, "an empty file takes an insert: {stdout}{stderr}");
            assert_eq!(
                read_file(path),
                "fn main()\n    println(\"ok\")",
                "the file opens with the declaration, not with whitespace"
            );
        });
    });
}

// A batch applies in order, so a later insert may call an earlier one.
#[test]
fn test_a_batch_of_inserts_applies_in_order() {
    with_source("fn base() int\n    return 1\n", |path| {
        with_body("fn one() int\n    return base() + 1", |first| {
            with_body("fn two() int\n    return one() + 1", |second| {
                let (stdout, stderr, ok) = patch(&[
                    "--insert-fn",
                    "one",
                    "--body-file",
                    first,
                    "--insert-fn",
                    "two",
                    "--body-file",
                    second,
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(ok, "a batch of inserts applies: {stdout}{stderr}");
                assert_eq!(
                    read_file(path),
                    "fn base() int\n    return 1\n\nfn one() int\n    return base() + 1\n\nfn two() int\n    return one() + 1\n",
                    "both declarations are written, and the second one sees the first"
                );
            })
        });
    });
}

// A declaration carrying a doc comment and an attribute keeps both when the
// insert lands after it.
#[test]
fn test_insert_after_a_decorated_declaration_leaves_its_decoration_attached() {
    let source = "// What this one does.\n@deprecated(\"use later instead\")\nfn earlier() int\n    return 1\n\nfn main()\n    println(\"ok\")\n";
    with_source(source, |path| {
        with_body("fn later() int\n    return 2", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "later",
                "--after",
                "earlier",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(
                ok,
                "an insert after a decorated declaration applies: {stdout}{stderr}"
            );
            assert_eq!(
                read_file(path),
                "// What this one does.\n@deprecated(\"use later instead\")\nfn earlier() int\n    return 1\n\nfn later() int\n    return 2\n\nfn main()\n    println(\"ok\")\n",
                "the comment and the attribute stay above the declaration they described"
            );
        });
    });
}

// Neither mode that holds a result back writes the file.
#[test]
fn test_insert_writes_nothing_in_check_only_or_dry_run() {
    for mode in ["--check-only", "--dry-run"] {
        with_source(PROBE, |path| {
            with_body("fn answer() int\n    return 42", |body| {
                let (stdout, stderr, ok) = patch(&[
                    "--insert-fn",
                    "answer",
                    "--body-file",
                    body,
                    mode,
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(
                    ok,
                    "{mode} reports the insert would apply: {stdout}{stderr}"
                );
                assert_eq!(read_file(path), PROBE, "{mode} writes nothing");
            });
        });
    }
}

// One call takes either a replacement or an insert, because both would pair
// against `--body-file` and the pairing would say nothing about which is which.
#[test]
fn test_a_replacement_and_an_insert_cannot_share_one_call() {
    with_source(PROBE, |path| {
        with_body("return 0", |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--replace-fn",
                "add",
                "--body-file",
                body,
                "--insert-fn",
                "answer",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "the two flags together are refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_012".to_string()),
                "the refusal carries MER_BLD_012: {stdout}"
            );
        });
    });
}

// An `--after` for some inserts but not others names no order, so it is refused.
#[test]
fn test_insert_refuses_a_partial_after_list() {
    with_source(PROBE, |path| {
        with_body("fn one() int\n    return 1", |first| {
            with_body("fn two() int\n    return 2", |second| {
                let (stdout, _stderr, ok) = patch(&[
                    "--insert-fn",
                    "one",
                    "--body-file",
                    first,
                    "--insert-fn",
                    "two",
                    "--body-file",
                    second,
                    "--after",
                    "add",
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(!ok, "a partial --after is refused: {stdout}");
                assert!(
                    codes(&envelope(&stdout)).contains(&"MER_BLD_012".to_string()),
                    "the refusal carries MER_BLD_012: {stdout}"
                );
            })
        });
    });
}

// Text declaring more than the one declaration asked for is refused, on both
// the top-level path and the method path.
//
// The method path is the one that needs saying: a container grows without the
// file's top-level count changing, so nothing downstream would notice the
// extra declaration and the caller would be told it inserted one thing.
#[test]
fn test_insert_refuses_text_declaring_more_than_one_declaration() {
    let two = "fn helper() int\n    return 1\n\nfn actual() int\n    return 2";

    let class_source = "class Order\n    total int\n\nfn main()\n    println(\"ok\")\n";
    with_source(class_source, |path| {
        with_body(two, |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "Order.actual",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "a body declaring two methods is refused: {stdout}");
            assert!(
                codes(&envelope(&stdout)).contains(&"MER_BLD_012".to_string()),
                "the refusal carries MER_BLD_012: {stdout}"
            );
            assert_eq!(read_file(path), class_source, "the file is left as it was");
        });
    });

    with_source(PROBE, |path| {
        with_body(two, |body| {
            let (stdout, _stderr, ok) = patch(&[
                "--insert-fn",
                "actual",
                "--body-file",
                body,
                "--format",
                "json",
                &path.display().to_string(),
            ]);
            assert!(!ok, "a body declaring two functions is refused: {stdout}");
            assert_eq!(read_file(path), PROBE, "the file is left as it was");
        });
    });
}

// A declaration reaches the command through standard input, which is what an
// agent holding text in memory rather than on disk uses.
#[test]
fn test_insert_reads_a_declaration_from_standard_input() {
    with_source(PROBE, |path| {
        let output = miri_cmd()
            .arg("patch")
            .args([
                "--insert-fn",
                "answer",
                "--body-file",
                "-",
                "--format",
                "json",
            ])
            .arg(path.display().to_string())
            .write_stdin("fn answer() int\n    return 42")
            .output()
            .expect("the patch command runs");

        assert!(
            output.status.success(),
            "a declaration read from standard input applies: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            read_file(path),
            "fn add(a int, b int) int\n    return a + b\n\nfn main()\n    println(\"ok\")\n\nfn answer() int\n    return 42\n",
            "the declaration read from standard input is the one written"
        );
    });
}

// A batch is refused whole. The first operation failing means the later ones
// never run and the file keeps every byte it had.
#[test]
fn test_a_batch_is_refused_whole_when_its_first_insert_is() {
    let source = "fn taken() int\n    return 1\n";
    with_source(source, |path| {
        with_body("fn taken() int\n    return 2", |first| {
            with_body("fn fresh() int\n    return 3", |second| {
                let (stdout, _stderr, ok) = patch(&[
                    "--insert-fn",
                    "taken",
                    "--body-file",
                    first,
                    "--insert-fn",
                    "fresh",
                    "--body-file",
                    second,
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(!ok, "the batch is refused: {stdout}");
                assert!(
                    codes(&envelope(&stdout)).contains(&"MER_BLD_017".to_string()),
                    "the refusal names the duplicate: {stdout}"
                );
                assert_eq!(
                    read_file(path),
                    source,
                    "neither operation reached the file"
                );
            })
        });
    });
}

// A container's members set the depth, whatever width the file indents with.
#[test]
fn test_insert_adopts_the_containers_own_indent_width() {
    for (width, indent) in [(2, "  "), (8, "        ")] {
        let source = format!(
            "class Tiny\n{indent}size int\n\nfn main()\n{indent}println(\"ok\")\n",
            indent = indent
        );
        with_source(&source, |path| {
            with_body("fn grow() int\n    return self.size + 1", |body| {
                let (stdout, stderr, ok) = patch(&[
                    "--insert-fn",
                    "Tiny.grow",
                    "--body-file",
                    body,
                    "--format",
                    "json",
                    &path.display().to_string(),
                ]);
                assert!(ok, "a {width}-space file takes a method: {stdout}{stderr}");
                assert!(
                    read_file(path).contains(&format!("\n\n{indent}fn grow() int\n")),
                    "the method header sits at the container's own member depth, not at four spaces: {}",
                    read_file(path)
                );
            });
        });
    }
}

// A dry run reports the difference it would make and writes nothing.
#[test]
fn test_insert_dry_run_reports_the_difference_without_writing() {
    with_source(PROBE, |path| {
        with_body("fn answer() int\n    return 42", |body| {
            let (stdout, stderr, ok) = patch(&[
                "--insert-fn",
                "answer",
                "--body-file",
                body,
                "--dry-run",
                &path.display().to_string(),
            ]);
            assert!(ok, "a dry run of an insert succeeds: {stdout}{stderr}");
            assert!(
                stdout.contains("+fn answer() int"),
                "the diff shows the declaration being added: {stdout}"
            );
            assert_eq!(read_file(path), PROBE, "a dry run writes nothing");
        });
    });
}
