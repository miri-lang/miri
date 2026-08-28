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
