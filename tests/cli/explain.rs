// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
use miri::diagnostics::DiagnosticCode;

/// Run `miri explain` and return (stdout, stderr, success).
fn explain(args: &[&str]) -> (String, String, bool) {
    let mut cmd = miri_cmd();
    let output = cmd
        .arg("explain")
        .args(args)
        .output()
        .expect("failed to run the explain command");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Parse an envelope out of the command's stdout.
fn envelope(stdout: &str) -> DiagnosticsEnvelope {
    serde_json::from_str(stdout).expect("explain did not emit a parseable envelope")
}

#[test]
fn test_explain_known_code_prints_body_and_succeeds() {
    let (stdout, _, ok) = explain(&["MER_TYP_030"]);
    assert!(ok, "explaining a live code must exit 0");
    assert!(stdout.contains("MER_TYP_030"), "output names the code");
    assert!(
        stdout.contains("Argument Count Mismatch"),
        "output carries the registry title, got: {}",
        stdout
    );
    assert!(stdout.contains("Rule"), "output carries the rule section");
    for section in ["Rule", "Before", "After", "Reference"] {
        assert!(
            stdout.contains(section),
            "a live code renders every section; {} is missing from:\n{}",
            section,
            stdout
        );
    }
}

#[test]
fn test_explain_pretty_marks_a_retired_code_in_the_body() {
    let (stdout, _, ok) = explain(&["MER_TYP_010"]);
    assert!(ok);
    assert!(
        stdout.contains("retired") || stdout.contains("no longer emitted"),
        "a reader must see that the code is retired, not just its old rule:\n{}",
        stdout
    );
}

#[test]
fn test_explain_color_is_controlled_by_the_flag() {
    const ESCAPE: &str = "\u{1b}[";

    let mut always = miri_cmd();
    let forced = always
        .args(["--color", "always", "explain", "MER_TYP_030"])
        .output()
        .expect("failed to run the explain command");
    assert!(
        String::from_utf8_lossy(&forced.stdout).contains(ESCAPE),
        "--color always must emit ANSI codes"
    );

    let mut never = miri_cmd();
    let plain = never
        .args(["--color", "never", "explain", "MER_TYP_030"])
        .output()
        .expect("failed to run the explain command");
    assert!(
        !String::from_utf8_lossy(&plain.stdout).contains(ESCAPE),
        "--color never must emit no ANSI codes"
    );

    // JSON is consumed by tools, so it stays free of escapes even when colour
    // is forced on.
    let mut json = miri_cmd();
    let structured = json
        .args([
            "--color",
            "always",
            "explain",
            "MER_TYP_030",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run the explain command");
    assert!(
        !String::from_utf8_lossy(&structured.stdout).contains(ESCAPE),
        "JSON output must never carry ANSI codes, even with --color always"
    );
}

#[test]
fn test_explain_json_round_trips_through_the_envelope() {
    let (stdout, _, ok) = explain(&["MER_TYP_030", "--format", "json"]);
    assert!(ok);
    let parsed = envelope(&stdout);

    assert_eq!(parsed.schema_version, 1);
    assert!(parsed.ok);
    assert_eq!(parsed.command, JsonCommand::Explain);
    assert!(
        parsed.diagnostics.is_empty(),
        "explaining a code is not itself a diagnosis"
    );

    let explanation = parsed
        .explanation
        .expect("the explain envelope must carry an explanation");
    assert_eq!(explanation.code, "MER_TYP_030");
    assert_eq!(explanation.severity, "error");
    assert!(!explanation.reserved);
    assert!(!explanation.rule.is_empty());
    assert!(explanation.example_before.is_some());
    assert!(explanation.example_after.is_some());
}

#[test]
fn test_explain_reserved_code_succeeds_and_is_marked_retired() {
    let (stdout, _, ok) = explain(&["MER_TYP_010", "--format", "json"]);
    assert!(ok, "a retired code is still explainable");

    let explanation = envelope(&stdout)
        .explanation
        .expect("a retired code still carries an explanation");
    assert!(explanation.reserved, "the retirement must be visible");
    assert!(
        explanation.example_before.is_none() && explanation.example_after.is_none(),
        "a retired check has no reproduction, so it carries no example pair"
    );
    assert!(
        explanation.rule.contains("MER_TYP_030"),
        "a retired code names its successor so a reader is not left stranded"
    );
}

#[test]
fn test_explain_unknown_code_fails_with_a_coded_diagnostic() {
    let (_, stderr, ok) = explain(&["MER_ZZZ_999"]);
    assert!(!ok, "an unknown code must exit non-zero");
    assert!(
        stderr.contains("MER_BLD_001"),
        "the failure is itself addressable by a code, got: {}",
        stderr
    );
}

#[test]
fn test_explain_unknown_code_json_reports_the_diagnostic() {
    let (stdout, _, ok) = explain(&["not-a-code", "--format", "json"]);
    assert!(!ok);

    let parsed = envelope(&stdout);
    assert!(!parsed.ok);
    assert_eq!(parsed.command, JsonCommand::Explain);
    assert!(parsed.explanation.is_none());
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(
        parsed.diagnostics[0].code.as_deref(),
        Some("MER_BLD_001"),
        "an unrecognised argument is reported through the registry, not as bare text"
    );
}

#[test]
fn test_explain_explains_its_own_unknown_code_diagnostic() {
    let (stdout, _, ok) = explain(&["MER_BLD_001"]);
    assert!(
        ok,
        "the code reporting an unknown code must itself be explainable"
    );
    assert!(stdout.contains("MER_BLD_001"));
}

#[test]
fn test_explain_every_registered_code_exits_zero_with_a_body() {
    for code in DiagnosticCode::all() {
        let wire = code.as_str();
        let (stdout, stderr, ok) = explain(&[wire, "--format", "json"]);
        assert!(ok, "explaining {} must exit 0, stderr: {}", wire, stderr);

        let explanation = envelope(&stdout)
            .explanation
            .unwrap_or_else(|| panic!("{} produced no explanation", wire));
        assert_eq!(
            explanation.code, wire,
            "explaining {} returned the explanation for {}",
            wire, explanation.code
        );
        assert_eq!(
            explanation.reserved,
            code.is_reserved(),
            "{} reports a retirement status that disagrees with the registry",
            wire
        );
        assert!(
            !explanation.title.trim().is_empty(),
            "{} has an empty title",
            wire
        );
        assert!(
            matches!(explanation.severity.as_str(), "error" | "warning" | "note"),
            "{} reports an unrecognised severity: {}",
            wire,
            explanation.severity
        );
        assert!(
            !explanation.rule.trim().is_empty(),
            "{} has an empty rule body",
            wire
        );
    }
}

#[test]
fn test_explain_help() {
    let mut cmd = miri_cmd();
    cmd.arg("explain")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Explain a diagnostic code"));
}

#[test]
fn test_explain_does_not_echo_terminal_escapes_from_its_argument() {
    let hostile = "X\u{1b}[31mINJECTED\u{1b}[0m";
    let (_, stderr, ok) = explain(&[hostile]);
    assert!(!ok, "a hostile argument is still just an unknown code");
    assert!(
        !stderr.contains('\u{1b}'),
        "the rejected argument is quoted back to the user, so it must not carry \
         escape sequences that repaint the terminal: {:?}",
        stderr
    );
    assert!(
        stderr.contains("INJECTED"),
        "the argument is still shown, just neutralised: {}",
        stderr
    );
}
