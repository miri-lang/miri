// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use miri::diagnostics::json::DiagnosticsEnvelope;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const VALID_MAIN: &str = r#"fn main() int
    42
"#;

const TYPE_ERROR_CODE: &str = r#"fn main() int
    if 42
        100
    200
"#;

const DEPRECATED_FUNCTION: &str = r#"@deprecated("use new_name instead")
fn old_name() int
    1

fn main() int
    old_name()
"#;

#[test]
fn test_check_format_json_valid() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["command"], "check");
    assert!(parsed["diagnostics"].is_array());
    assert_eq!(parsed["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn test_check_format_json_with_error() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "check");
    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());

    let first_diag = &diags[0];
    assert_eq!(first_diag["severity"], "error");
    assert!(first_diag["message"].is_string());
    // Verify code is a string (diagnostic code like "MER_TYP_002")
    assert!(
        first_diag["code"].is_string(),
        "code should be a string, got: {:?}",
        first_diag["code"]
    );
    // Verify line and column are present and valid
    assert!(first_diag["line"].is_number(), "line should be a number");
    assert!(
        first_diag["column"].is_number(),
        "column should be a number"
    );
    assert!(
        first_diag["length"].is_number(),
        "length should be a number"
    );
}

#[test]
fn test_check_format_json_no_ansi() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("\x1b"),
        "JSON output should not contain ANSI escape codes"
    );
}

#[test]
fn test_build_format_json_success() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("build")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["command"], "build");
    assert!(parsed["artifact"].is_string());
    assert!(!parsed["artifact"].as_str().unwrap().is_empty());
}

#[test]
fn test_check_format_pretty_default() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd().arg("check").arg(path).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    // Pretty format should not parse as JSON
    let result = serde_json::from_str::<serde_json::Value>(&stdout);
    assert!(result.is_err(), "Default format should not be JSON");
}

#[test]
fn test_check_format_json_warnings_ok_true() {
    let file = create_test_file(DEPRECATED_FUNCTION);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    // Critical: warnings should NOT make ok false
    assert_eq!(
        parsed["ok"], true,
        "ok should be true when check succeeds, even with warnings"
    );
    assert_eq!(
        parsed["exitCode"], 0,
        "exitCode should be 0 when check succeeds"
    );
    assert_eq!(parsed["command"], "check");

    // Verify the warning is present
    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty(), "should have at least one warning");
    assert_eq!(
        diags[0]["severity"], "warning",
        "first diagnostic should be a warning"
    );

    // Verify process exit code is 0
    assert_eq!(output.status.code(), Some(0), "process should exit with 0");
}

#[test]
fn test_check_format_json_error_exit_code_one() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["ok"], false, "ok should be false when check fails");
    assert_eq!(
        parsed["exitCode"], 1,
        "exitCode should be 1 when check fails"
    );

    // Verify process exit code is 1
    assert_eq!(output.status.code(), Some(1), "process should exit with 1");
}

#[test]
fn test_check_format_json_multiple_errors() {
    let multi_error = r#"fn main() int
    if 42
        100
    if true
        200
"#;
    let file = create_test_file(multi_error);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(diags.len() > 1, "should have multiple diagnostics");

    // Verify all errors have actual values (not just is_string() check)
    for (i, diag) in diags.iter().enumerate() {
        assert_eq!(
            diag["severity"], "error",
            "diagnostic {} should be an error",
            i
        );
        assert!(
            diag["message"].is_string(),
            "diagnostic {} should have a message",
            i
        );
    }
}

#[test]
fn test_run_format_json_compile_error() {
    let compile_error = r#"fn f() int
    return "hello"

fn main() int
    return f()
"#;
    let file = create_test_file(compile_error);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(parsed["ok"], false, "ok should be false for compile error");
    assert_eq!(parsed["command"], "run");
    assert_eq!(
        parsed["exitCode"], 1,
        "exitCode should be 1 for compile error"
    );

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1, "should have exactly one diagnostic");

    let first_diag = &diags[0];
    assert_eq!(
        first_diag["severity"], "error",
        "diagnostic should be error severity"
    );
    assert_eq!(
        first_diag["code"], "MER_TYP_002",
        "diagnostic code should be MER_TYP_002"
    );
    assert!(first_diag["message"].is_string());
    assert_eq!(first_diag["line"], 2, "error should be on line 2");
    assert_eq!(first_diag["column"], 12, "error should be at column 12");

    assert!(stderr.is_empty(), "stderr should be empty in JSON mode");
    assert_eq!(output.status.code(), Some(1), "process should exit with 1");
}

#[test]
fn test_run_format_json_runtime_failure() {
    let runtime_fail = r#"fn main() int
    42
"#;
    let file = create_test_file(runtime_fail);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(
        parsed["ok"], false,
        "ok should be false when program returns non-zero"
    );
    assert_eq!(parsed["command"], "run");
    assert_eq!(
        parsed["exitCode"], 42,
        "exitCode should match program exit code"
    );

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(
        diags.is_empty(),
        "diagnostics should be empty for runtime failure"
    );
    assert_eq!(
        output.status.code(),
        Some(42),
        "process should exit with 42"
    );
}

#[test]
fn test_run_format_json_division_by_zero_trap() {
    let div_by_zero = r#"fn main() int
    var x = 0
    10 / x
"#;
    let file = create_test_file(div_by_zero);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(
        parsed["ok"], false,
        "ok should be false when division by zero trap occurs"
    );
    assert_eq!(parsed["command"], "run");
    assert_eq!(
        parsed["exitCode"], 1,
        "exitCode should be 1 for runtime trap"
    );

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert_eq!(
        diags.len(),
        1,
        "should have exactly one diagnostic for division by zero"
    );

    let trap_diag = &diags[0];
    assert_eq!(trap_diag["severity"], "error");
    assert_eq!(
        trap_diag["code"], "MER_RT_001",
        "code should be MER_RT_001 for division by zero"
    );
    assert_eq!(trap_diag["message"], "division by zero");

    assert_eq!(output.status.code(), Some(1), "process should exit with 1");
}

#[test]
fn test_run_format_json_remainder_by_zero_trap() {
    let rem_by_zero = r#"fn main() int
    var x = 0
    10 % x
"#;
    let file = create_test_file(rem_by_zero);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(
        parsed["ok"], false,
        "ok should be false when remainder by zero trap occurs"
    );
    assert_eq!(parsed["command"], "run");
    assert_eq!(
        parsed["exitCode"], 1,
        "exitCode should be 1 for runtime trap"
    );

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert_eq!(
        diags.len(),
        1,
        "should have exactly one diagnostic for remainder by zero"
    );

    let trap_diag = &diags[0];
    assert_eq!(trap_diag["severity"], "error");
    assert_eq!(
        trap_diag["code"], "MER_RT_002",
        "code should be MER_RT_002 for remainder by zero"
    );
    assert_eq!(trap_diag["message"], "remainder by zero");

    assert_eq!(output.status.code(), Some(1), "process should exit with 1");
}

// Round-trip end-to-end tests that verify the real binary emits valid JSON
// without any unknown fields (enforced by deny_unknown_fields on the DTOs)

#[test]
fn test_check_json_round_trip_success() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert!(envelope.ok);
    assert_eq!(
        envelope.command,
        miri::diagnostics::json::JsonCommand::Check
    );
}

#[test]
fn test_check_json_round_trip_error() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("check")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert!(!envelope.ok);
    assert_eq!(
        envelope.command,
        miri::diagnostics::json::JsonCommand::Check
    );
    assert!(!envelope.diagnostics.is_empty());
}

#[test]
fn test_build_json_round_trip_success() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("build")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert!(envelope.ok);
    assert_eq!(
        envelope.command,
        miri::diagnostics::json::JsonCommand::Build
    );
    assert!(envelope.artifact.is_some());
}

#[test]
fn test_build_json_round_trip_error() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("build")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert!(!envelope.ok);
    assert_eq!(
        envelope.command,
        miri::diagnostics::json::JsonCommand::Build
    );
    assert!(!envelope.diagnostics.is_empty());
}

#[test]
fn test_run_json_round_trip_success() {
    let file = create_test_file(VALID_MAIN);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert_eq!(envelope.command, miri::diagnostics::json::JsonCommand::Run);
    assert!(envelope.exit_code.is_some());
}

#[test]
fn test_run_json_round_trip_error() {
    let file = create_test_file(TYPE_ERROR_CODE);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert!(!envelope.ok);
    assert_eq!(envelope.command, miri::diagnostics::json::JsonCommand::Run);
    assert!(!envelope.diagnostics.is_empty());
}

#[test]
fn test_test_json_round_trip() {
    let output = miri_cmd()
        .arg("test")
        .arg("--format")
        .arg("json")
        .arg("--dir")
        .arg(".")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let envelope: DiagnosticsEnvelope = serde_json::from_str(&stdout)
        .expect("JSON output should deserialize to DiagnosticsEnvelope without unknown fields");

    assert_eq!(envelope.schema_version, 1);
    assert_eq!(envelope.command, miri::diagnostics::json::JsonCommand::Test);
    assert!(envelope.tests.is_some());
}
