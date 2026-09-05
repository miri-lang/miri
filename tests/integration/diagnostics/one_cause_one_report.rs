// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! One cause should produce one report.
//!
//! Writing `Ok(n):` where `Result.Ok(n):` is required is a single mistake, but
//! it used to produce five diagnostics: the pattern failed to resolve, so the
//! names it binds were never defined, so the arm bodies referring to them were
//! reported undefined, and the match was reported non-exhaustive because no
//! variant had been covered. Two of the five carried a `Did you mean` line
//! naming something from the enclosing scope, which points away from the fix.
//!
//! These tests pin the shape an agent consumes: one primary diagnostic, the
//! echoes attached to it as `related` entries that carry their own location.

use miri::diagnostics::json::DiagnosticsEnvelope;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// A `match` whose arms both omit the `Result.` prefix the patterns require.
const BARE_RESULT_PATTERNS: &str = r#"fn classify(r Result<int, String>) int
    match r
        Ok(n): inv(n)
        Err(e): e.length()

fn inv(x int) int
    x

fn main()
    println(f"{classify(Result.Ok(1))}")
"#;

/// The envelope `miri check --format json` produces for `source`.
fn check_envelope(source: &str) -> DiagnosticsEnvelope {
    let mut file = NamedTempFile::new().expect("a temp file for the program");
    write!(file, "{}", source).expect("the program is written");
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    let output = crate::utils::miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib.to_str().expect("a utf-8 path"))
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .arg("check")
        .arg(file.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("check runs");

    let stdout = String::from_utf8(output.stdout).expect("utf-8 output");
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("a parseable envelope: {e}\n{stdout}"))
}

#[test]
fn test_a_failed_pattern_does_not_report_the_names_it_binds_as_undefined() {
    let envelope = check_envelope(BARE_RESULT_PATTERNS);
    let undefined: Vec<_> = envelope
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined variable"))
        .map(|d| d.message.clone())
        .collect();
    assert!(
        undefined.is_empty(),
        "a pattern that did not resolve still binds its names, so the arm body \
         must not be reported against the enclosing scope: {undefined:?}"
    );
}

#[test]
fn test_the_bare_pattern_program_reports_one_primary_diagnostic() {
    let envelope = check_envelope(BARE_RESULT_PATTERNS);
    let messages: Vec<_> = envelope
        .diagnostics
        .iter()
        .map(|d| format!("{:?} {}", d.code, d.message))
        .collect();
    assert_eq!(
        envelope.diagnostics.len(),
        1,
        "one mistake should produce one report: {messages:#?}"
    );
}

#[test]
fn test_the_primary_diagnostic_names_the_fix() {
    let envelope = check_envelope(BARE_RESULT_PATTERNS);
    let primary = &envelope.diagnostics[0];
    assert_eq!(primary.code.as_deref(), Some("MER_TYP_038"));
    assert!(
        primary.message.contains("Result.Ok"),
        "the report should spell the prefix the pattern is missing: {}",
        primary.message
    );
}

#[test]
fn test_the_second_bad_arm_is_attached_as_a_located_related_entry() {
    let envelope = check_envelope(BARE_RESULT_PATTERNS);
    let primary = &envelope.diagnostics[0];
    assert_eq!(
        primary.related.len(),
        1,
        "the other arm with the same mistake belongs on the same report: {:#?}",
        primary.related
    );
    let related = &primary.related[0];
    assert!(
        related.message.contains("Result.Err"),
        "the related entry should name the second arm's fix: {}",
        related.message
    );
    assert_eq!(
        related.line,
        Some(4),
        "a related entry without a line cannot be acted on: {related:#?}"
    );
    assert!(
        related.path.is_some(),
        "a related entry must say which file it is in: {related:#?}"
    );
}

/// The match is not reported non-exhaustive: every variant *was* written, and
/// telling an author to add an `Err` arm they already wrote sends them the
/// wrong way.
#[test]
fn test_a_match_whose_patterns_did_not_resolve_is_not_also_called_non_exhaustive() {
    let envelope = check_envelope(BARE_RESULT_PATTERNS);
    let exhaustiveness: Vec<_> = envelope
        .diagnostics
        .iter()
        .flat_map(|d| {
            std::iter::once(d.message.clone()).chain(d.related.iter().map(|r| r.message.clone()))
        })
        .filter(|m| m.contains("Non-exhaustive"))
        .collect();
    assert!(
        exhaustiveness.is_empty(),
        "the arms were written; they just failed to resolve: {exhaustiveness:?}"
    );
}

/// A correctly written match still reports what it genuinely misses, so the
/// suppression above cannot hide a real non-exhaustive match.
#[test]
fn test_a_resolved_match_that_is_genuinely_non_exhaustive_is_still_reported() {
    let code = r#"enum Color
    Red
    Green
    Blue

fn describe(c Color) String
    match c
        Color.Red: "red"
        Color.Green: "green"

fn main()
    println(describe(Color.Red))
"#;
    let envelope = check_envelope(code);
    assert!(
        envelope
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Missing variants: Blue")),
        "a match that resolved and still misses a variant is a real error: {:#?}",
        envelope.diagnostics
    );
}

/// Three arms with the same mistake stay one report, so the count of related
/// entries tracks the echoes rather than being fixed at one.
#[test]
fn test_every_repeat_of_the_mistake_hangs_off_the_same_report() {
    let code = r#"enum Signal
    Start(int)
    Stop(int)
    Reset(int)

fn handle(s Signal) int
    match s
        Start(x): x
        Stop(x): x
        Reset(x): x

fn main()
    println(f"{handle(Signal.Start(1))}")
"#;
    let envelope = check_envelope(code);
    assert_eq!(envelope.diagnostics.len(), 1);
    let lines: Vec<_> = envelope.diagnostics[0]
        .related
        .iter()
        .map(|r| r.line)
        .collect();
    assert_eq!(
        lines,
        vec![Some(9), Some(10)],
        "each repeat keeps its own line: {:#?}",
        envelope.diagnostics[0].related
    );
}
