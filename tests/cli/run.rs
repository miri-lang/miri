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

/// Deliberate non-zero exit (fn main() int) must not add spurious stderr noise.
#[test]
fn test_deliberate_nonzero_exit_no_noise() {
    let code = r#"fn main() int
    3
"#;
    let file = create_test_file(code);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd().arg("run").arg(path).output().unwrap();

    // Exit code should be 3
    assert_eq!(output.status.code(), Some(3));
    // No stderr noise about termination
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("signal"),
        "deliberate exit should not print signal message to stderr, got: {}",
        stderr
    );
}

/// A program that unbounded recursion drives into a stack overflow, which the
/// operating system answers with SIGSEGV. The compiler cannot see this coming,
/// so the only useful report is the one the driver makes after the child dies.
const SIGSEGV_PROGRAM: &str = r#"fn deep(n int) int
    return deep(n + 1)

fn main()
    let x = deep(1)
    println(f"{x}")
"#;

/// A signal death used to reach the caller as an empty envelope: no code, no
/// message, no signal, and an exit code of -1 that could not be told apart from
/// a program that chose to fail. Every one of those fields is asserted here
/// because each was wrong before.
#[test]
fn test_signal_death_reports_a_coded_diagnostic_in_json() {
    let file = create_test_file(SIGSEGV_PROGRAM);
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

    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "run");
    assert_eq!(parsed["signal"], 11);
    assert_eq!(parsed["exitCode"], 139);
    assert_eq!(parsed["diagnostics"][0]["code"], "MER_RT_006");
    assert_eq!(
        parsed["diagnostics"][0]["message"],
        "terminated by signal 11 (SIGSEGV)"
    );
    assert_eq!(output.status.code(), Some(139));
}

/// The pretty path carries the same sentence. Before this, a segfault left the
/// terminal completely silent and exited 255, which said nothing about either
/// the cause or the signal.
#[test]
fn test_signal_death_names_the_signal_on_stderr_in_pretty_mode() {
    let file = create_test_file(SIGSEGV_PROGRAM);
    let path = file.path().to_str().unwrap();

    let output = miri_cmd().arg("run").arg(path).output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("terminated by signal 11 (SIGSEGV)"),
        "a signal death must name the signal on stderr, got: {}",
        stderr
    );
    assert_eq!(output.status.code(), Some(139));
}
