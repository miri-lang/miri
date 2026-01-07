// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

const SIMPLE_MAIN: &str = r#"fn main() int
    42
"#;

#[test]
fn test_build_valid_file() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .assert()
        .success()
        .stdout(predicates::str::contains("Build successful"));
}

#[test]
fn test_build_with_output() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let out_file = NamedTempFile::new().unwrap();
    let out_path = out_file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--out")
        .arg(out_path)
        .assert()
        .success();
}

#[test]
fn test_build_release() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--release")
        .assert()
        .success();
}

#[test]
fn test_build_opt_level() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--opt-level")
        .arg("3")
        .assert()
        .success();
}

#[test]
fn test_build_invalid_opt_level() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--opt-level")
        .arg("4")
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid value '4'"));
}

#[test]
fn test_build_cpu_backend_cranelift() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--cpu-backend")
        .arg("cranelift")
        .assert()
        .success();
}

#[test]
fn test_build_cpu_backend_llvm() {
    let file = create_test_file(SIMPLE_MAIN);
    let path = file.path().to_str().unwrap();

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(path)
        .arg("--cpu-backend")
        .arg("llvm")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "LLVM backend is not yet supported",
        ));
}
