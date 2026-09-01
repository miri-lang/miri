// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

// Helper to create a test file with a main function
fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const SIMPLE_MAIN: &str = r#"fn main() int
    0
"#;

#[test]
fn test_run_valid_file() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run").arg(path).assert().success();
}

#[test]
fn test_run_file_not_found() {
    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg("non_existent_file.mi")
        .assert()
        .failure()
        .stderr(predicates::str::contains("MER_BLD_008"))
        .stderr(predicates::str::contains("could not read"));
}

/// A path that names a directory is a coded diagnostic, not an unhandled
/// operating-system error: the caller pointed the command at a project instead
/// of a file, and the help line is what says so.
#[test]
fn test_run_directory_is_reported_with_a_code_and_help() {
    let directory = tempfile::tempdir().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg(directory.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("MER_BLD_008"))
        .stderr(predicates::str::contains("it is a directory, not a file"))
        .stderr(predicates::str::contains("name a single .mi file"));
}

/// The same failure answers a machine in the shape every other command
/// promises, rather than as a bare line of prose on stderr.
#[test]
fn test_run_directory_reports_an_envelope_in_json() {
    let directory = tempfile::tempdir().unwrap();

    let output = miri_cmd()
        .arg("run")
        .arg(directory.path())
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "run");
    assert_eq!(parsed["exitCode"], 1);
    assert_eq!(parsed["diagnostics"][0]["code"], "MER_BLD_008");
    assert!(parsed["diagnostics"][0]["help"].is_string());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn test_run_with_args() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg(path)
        .arg("--")
        .arg("arg1")
        .arg("arg2")
        .assert()
        .success();
}

#[test]
fn test_run_runtime_error() {
    let mut file = NamedTempFile::new().unwrap();
    // Invalid syntax
    write!(file, "let x = ").unwrap();
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run").arg(path).assert().failure();
}
