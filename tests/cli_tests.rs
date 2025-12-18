// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use assert_cmd::Command;

#[test]
fn test_version() {
    let mut cmd = Command::cargo_bin("miri").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout("miri 0.1.0\n");
}

#[test]
fn test_help() {
    let mut cmd = Command::cargo_bin("miri").unwrap();
    cmd.arg("--help").assert().success();
}
