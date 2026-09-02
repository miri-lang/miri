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

/// Every registered code, as the listing reports it.
fn listed_codes() -> Vec<miri::diagnostics::json::JsonCode> {
    let output = miri_cmd()
        .args(["explain", "--list", "--format", "json"])
        .output()
        .expect("the listing runs");
    assert!(
        output.status.success(),
        "listing the registry should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: DiagnosticsEnvelope =
        serde_json::from_str(&stdout).expect("the listing is a parseable envelope");
    assert!(envelope.ok, "a listing reports success");
    assert_eq!(envelope.exit_code, Some(0), "a listing exits zero");
    envelope.codes.expect("the listing carries the registry")
}

#[test]
fn test_list_json_covers_the_whole_registry() {
    let listed = listed_codes();
    assert_eq!(
        listed.len(),
        DiagnosticCode::all().len(),
        "the listing carries every registered code"
    );
    let listed_names: Vec<&str> = listed.iter().map(|entry| entry.code.as_str()).collect();
    let registry_names: Vec<&str> = DiagnosticCode::all()
        .iter()
        .map(|code| code.as_str())
        .collect();
    assert_eq!(
        listed_names, registry_names,
        "the listing keeps the registry's order"
    );
}

#[test]
fn test_list_json_carries_every_field_for_every_code() {
    for entry in listed_codes() {
        assert!(!entry.code.is_empty(), "a listed code names itself");
        assert!(!entry.title.is_empty(), "{} carries a title", entry.code);
        // A non-empty string is not enough: a value outside the registry's own
        // vocabulary would still read as present to a consumer switching on it.
        assert!(
            matches!(entry.severity.as_str(), "error" | "warning" | "note"),
            "{} reports a severity the schema admits, got {}",
            entry.code,
            entry.severity
        );
        assert!(
            matches!(
                entry.fix_safety.as_str(),
                "format-only"
                    | "behavior-preserving"
                    | "local-edit"
                    | "api-changing"
                    | "target-changing"
                    | "requires-human-review"
            ),
            "{} reports a fix-safety the schema admits, got {}",
            entry.code,
            entry.fix_safety
        );
        assert!(
            entry.code.starts_with("MER_") && entry.code.contains(&format!("_{}_", entry.area)),
            "{} is spelled as a registry code naming its own area {}",
            entry.code,
            entry.area
        );
    }
}

#[test]
fn test_list_json_reports_retirement_from_the_registry() {
    let listed = listed_codes();
    for code in DiagnosticCode::all() {
        let entry = listed
            .iter()
            .find(|entry| entry.code == code.as_str())
            .unwrap_or_else(|| panic!("{} appears in the listing", code.as_str()));
        assert_eq!(
            entry.retired,
            code.is_reserved(),
            "{} reports its retirement as the registry holds it",
            code.as_str()
        );
        assert_eq!(
            entry.severity,
            code.severity().as_str(),
            "{} reports the registry's severity",
            code.as_str()
        );
        assert_eq!(
            entry.area,
            code.area(),
            "{} reports the registry's area",
            code.as_str()
        );
        assert_eq!(
            entry.fix_safety,
            code.fix_safety().as_str(),
            "{} reports the registry's fix-safety",
            code.as_str()
        );
    }
    assert!(
        listed.iter().any(|entry| entry.retired),
        "the registry holds at least one retired code, and the listing shows it"
    );
    assert!(
        listed.iter().any(|entry| !entry.retired),
        "the registry holds live codes, and the listing shows them"
    );
}

#[test]
fn test_list_pretty_names_severity_and_fix_safety() {
    let output = miri_cmd()
        .args(["explain", "--list"])
        .output()
        .expect("the listing runs");
    assert!(
        output.status.success(),
        "listing the registry should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().count(),
        DiagnosticCode::all().len(),
        "one line per registered code"
    );
    let first = stdout.lines().next().expect("the listing is not empty");
    let head = DiagnosticCode::all()
        .first()
        .expect("the registry is not empty");
    assert!(first.contains(head.as_str()), "a row names its code");
    assert!(
        first.contains(head.severity().as_str()),
        "a row names its severity, got: {first}"
    );
    assert!(
        first.contains(head.fix_safety().as_str()),
        "a row names its fix-safety, got: {first}"
    );
    assert!(first.contains(head.title()), "a row names its title");
}

#[test]
fn test_explain_without_a_code_or_list_names_both() {
    let output = miri_cmd()
        .arg("explain")
        .output()
        .expect("the command runs");
    assert!(
        !output.status.success(),
        "explain needs something to explain"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CODE") && stderr.contains("--list"),
        "the failure names both ways to ask, got: {stderr}"
    );
}

#[test]
fn test_a_code_and_list_are_mutually_exclusive() {
    let output = miri_cmd()
        .args(["explain", "MER_LEX_001", "--list"])
        .output()
        .expect("the command runs");
    assert!(
        !output.status.success(),
        "a code and a listing are different requests"
    );
}

#[test]
fn test_subcommand_help_does_not_repeat_the_global_flag_prose() {
    let output = miri_cmd()
        .args(["explain", "--help"])
        .output()
        .expect("the help runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Global options"),
        "the globals are grouped under their own heading, got: {stdout}"
    );
    assert!(
        !stdout.contains("StorageLive/Dead balance"),
        "a subcommand does not reprint the global flag's full prose, got: {stdout}"
    );

    let root = miri_cmd()
        .arg("--help")
        .output()
        .expect("the root help runs");
    let root_stdout = String::from_utf8_lossy(&root.stdout);
    assert!(
        root_stdout.contains("StorageLive/Dead balance"),
        "the full explanation still reaches a reader at the root, got: {root_stdout}"
    );
}
