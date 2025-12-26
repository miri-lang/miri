// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;

#[test]
fn test_repl_quit() {
    let mut cmd = miri_cmd();
    cmd.write_stdin(":quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("Miri REPL"));
}

#[test]
fn test_repl_exit() {
    let mut cmd = miri_cmd();
    cmd.write_stdin(":exit\n").assert().success();
}

#[test]
fn test_repl_q() {
    let mut cmd = miri_cmd();
    cmd.write_stdin(":q\n").assert().success();
}

#[test]
fn test_repl_expression() {
    let mut cmd = miri_cmd();
    cmd.write_stdin("let x = 1\n:quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("Execution successful"));
}
