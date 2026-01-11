// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

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
        .stderr(predicates::str::contains("Failed to read file"));
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
