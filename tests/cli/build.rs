// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_build_valid_file() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
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
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
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

    // Verify output file exists/was created (though NamedTempFile creates it, we might want to check content or timestamp if possible, but simple success is enough for CLI arg check)
}

#[test]
fn test_build_release() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
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
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
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
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "print(\"Hello, World!\")").unwrap();
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
