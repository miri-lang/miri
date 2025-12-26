// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;

#[test]
fn test_help_flag() {
    let mut cmd = miri_cmd();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Miri Compiler"))
        .stdout(predicates::str::contains("Usage:"));
}

#[test]
fn test_short_help_flag() {
    let mut cmd = miri_cmd();
    cmd.arg("-h")
        .assert()
        .success()
        .stdout(predicates::str::contains("Usage:"));
}

#[test]
fn test_run_help() {
    let mut cmd = miri_cmd();
    cmd.arg("run")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Run a Miri source file"));
}

#[test]
fn test_build_help() {
    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Build a Miri source file"));
}

#[test]
fn test_test_help() {
    let mut cmd = miri_cmd();
    cmd.arg("test")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("Run tests"));
}
