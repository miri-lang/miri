// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_run_valid_file() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg(path)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "AST generated with 1 statements.",
        ));
}

#[test]
fn test_run_file_not_found() {
    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg("non_existent_file.mi")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Failed to read file"));
}

#[test]
fn test_run_with_args() {
    // Assuming we can access args in the script later, but for now just checking it accepts them
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Args test\")").unwrap();
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg(path)
        .arg("--")
        .arg("arg1")
        .arg("arg2")
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "AST generated with 1 statements.",
        ));
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
