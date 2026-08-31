// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri fmt` — canonical formatter command.

use std::io::Write;
use std::path::Path;

use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};

use crate::utils::miri_cmd;

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

/// Run `miri fmt` and return (stdout, stderr, success).
fn fmt(args: &[&str]) -> (String, String, bool) {
    let output = miri_cmd()
        .arg("fmt")
        .args(args)
        .output()
        .expect("the fmt command runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// A file whose text is not its canonical form: the parentheses are redundant.
const UNFORMATTED: &str = "fn helper(a int, b int) int\n    let t = (a + b)\n    return t\n";

/// Parse the envelope out of a JSON run.
fn envelope(stdout: &str) -> DiagnosticsEnvelope {
    serde_json::from_str(stdout).expect("fmt emits a parseable envelope")
}

#[test]
fn test_fmt_normalizes_redundant_parentheses() {
    let source = "fn main()\n    let t = (a + b)\n";
    with_source(source, |path| {
        let (stdout, _, ok) = fmt(&[&path.display().to_string()]);
        assert!(ok, "fmt should succeed on valid code: {stdout}");
        let content = std::fs::read_to_string(path).expect("file can be read");
        assert!(
            content.contains("let t = a + b"),
            "parentheses should be removed"
        );
        assert!(!content.contains("let t = (a + b)"), "got: {content}");
    });
}

#[test]
fn test_fmt_is_idempotent() {
    // The fixture must NOT already be canonical, or both runs are no-ops and
    // the test would pass without formatting anything.
    with_source(UNFORMATTED, |path| {
        let (_, _, ok1) = fmt(&[&path.display().to_string()]);
        assert!(ok1, "first fmt should succeed");
        let content_after_first = std::fs::read_to_string(path).expect("file can be read");
        assert_ne!(
            content_after_first, UNFORMATTED,
            "the first run has something to do, or this proves nothing"
        );
        let (_, _, ok2) = fmt(&[&path.display().to_string()]);
        assert!(ok2, "second fmt should succeed");
        let content_after_second = std::fs::read_to_string(path).expect("file can be read");
        assert_eq!(
            content_after_first, content_after_second,
            "fmt should be idempotent"
        );
    });
}

#[test]
fn test_fmt_with_check_flag_does_not_write() {
    let source = "fn main()\n    let t = (a + b)\n";
    with_source(source, |path| {
        let (_, _, ok) = fmt(&[&path.display().to_string(), "--check"]);
        assert!(!ok, "fmt --check should fail on unformatted code");
        let content = std::fs::read_to_string(path).expect("file can be read");
        assert_eq!(content, source, "file should not be modified with --check");
    });
}

#[test]
fn test_fmt_with_check_flag_succeeds_on_canonical_code() {
    let source = "fn main()\n    let t = a + b\n";
    with_source(source, |path| {
        let (_, _, ok) = fmt(&[&path.display().to_string(), "--check"]);
        assert!(ok, "fmt --check should succeed on canonical code");
    });
}

#[test]
fn test_fmt_rejects_unparseable_file() {
    let source = "fn main((\n";
    with_source(source, |path| {
        let (_, stderr, ok) = fmt(&[&path.display().to_string()]);
        assert!(!ok, "fmt should fail on unparseable code");
        assert!(
            stderr.contains("Unexpected"),
            "should report parse error: {stderr}"
        );
        let content = std::fs::read_to_string(path).expect("file can be read");
        assert_eq!(
            content, source,
            "file should not be modified on parse error"
        );
    });
}

#[test]
fn test_fmt_json_output() {
    let source = "fn main()\n    let t = (a + b)\n";
    with_source(source, |path| {
        let (stdout, _, _) = fmt(&[&path.display().to_string(), "--format", "json"]);
        let envelope = envelope(&stdout);
        assert_eq!(envelope.command, JsonCommand::Fmt);
    });
}

#[test]
fn test_check_does_not_claim_it_rewrote_the_file() {
    with_source(UNFORMATTED, |path| {
        let (stdout, _, ok) = fmt(&[&path.display().to_string(), "--check"]);

        assert!(!ok, "a file that is not canonical fails --check");
        assert!(
            !stdout.contains("rewritten"),
            "--check writes nothing, so it must not report a rewrite: {stdout}"
        );
    });
}

#[test]
fn test_the_check_envelope_agrees_with_the_exit_status() {
    with_source(UNFORMATTED, |path| {
        let (stdout, _, ok) = fmt(&[&path.display().to_string(), "--check", "--format", "json"]);

        assert!(!ok, "a file that is not canonical fails --check");
        let envelope: serde_json::Value =
            serde_json::from_str(&stdout).expect("fmt emits a parseable envelope");
        assert_eq!(
            envelope["ok"], false,
            "the envelope may not report success while the command fails: {stdout}"
        );
        assert_eq!(
            envelope["exitCode"], 1,
            "the envelope's exit code is the one the process returns: {stdout}"
        );
    });
}

#[test]
fn test_fmt_does_not_delete_a_block_comment() {
    let source =
        "fn helper(a int, b int) int\n    /* why it adds */\n    let t = (a + b)\n    return t\n";
    with_source(source, |path| {
        let (_, stderr, ok) = fmt(&[&path.display().to_string()]);
        assert!(ok, "the file formats: {stderr}");

        let after = std::fs::read_to_string(path).expect("the formatted file is readable");
        assert!(
            after.contains("/* why it adds */"),
            "formatting rewrites the file, so it must not cost the author a comment: {after}"
        );
        assert!(
            !after.contains("(a + b)"),
            "the redundant parentheses are still normalized away: {after}"
        );
    });
}

#[test]
fn test_fmt_keeps_a_comment_that_no_statement_follows() {
    let source = "fn add(a int, b int) int\n    return a + b\n    // why it adds\n";
    with_source(source, |path| {
        let (_, stderr, ok) = fmt(&[&path.display().to_string()]);
        assert!(ok, "the file formats: {stderr}");

        let after = std::fs::read_to_string(path).expect("the formatted file is readable");
        assert!(
            after.contains("// why it adds"),
            "a comment with no statement below it is still the author's: {after}"
        );
    });
}

#[test]
fn test_fmt_refuses_a_file_whose_comments_it_cannot_carry() {
    // A file of only comments parses to an empty program, so rendering it
    // would produce an empty file. Refusing beats emptying the file.
    let source = "// a file that is only notes\n// second line\n";
    with_source(source, |path| {
        let (stdout, stderr, ok) = fmt(&[&path.display().to_string()]);

        assert!(!ok, "formatting must not silently drop the file's content");
        assert!(
            format!("{stdout}{stderr}").contains("MER_BLD_019"),
            "the refusal names the content it would have lost: {stdout}{stderr}"
        );
        assert_eq!(
            std::fs::read_to_string(path).expect("the file is readable"),
            source,
            "a refused format leaves the file exactly as it was"
        );
    });
}

#[test]
fn test_fmt_accepts_an_empty_file() {
    with_source("", |path| {
        let (_, stderr, ok) = fmt(&[&path.display().to_string()]);
        assert!(ok, "an empty file is already canonical: {stderr}");
        assert_eq!(
            std::fs::read_to_string(path).expect("the file is readable"),
            "",
            "an empty file is left empty"
        );
    });
}
