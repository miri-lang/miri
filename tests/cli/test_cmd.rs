// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::utils::miri_cmd;

#[test]
fn test_test_command_help() {
    let mut cmd = miri_cmd();
    cmd.arg("test").arg("--help").assert().success();
}

#[test]
fn test_test_command_default() {
    // We need to run tests in a controlled environment where we know the outcome.
    // Running `miri test` in the project root runs all tests, including examples which might fail.
    // So we create a temporary directory with a passing test file.

    use std::io::Write;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_pass.mi");
    let mut file = std::fs::File::create(&file_path).unwrap();
    write!(file, "print(\"Hello\")").unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--dir")
        .arg(temp_dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test result: ok"));
}
