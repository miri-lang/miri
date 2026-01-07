// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::test_utils::miri_cmd;

#[test]
fn test_repl_quit() {
    let mut cmd = miri_cmd();
    cmd.write_stdin(":quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("Miri 0.1.0"));
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
    // Test that expressions return values
    let mut cmd = miri_cmd();
    cmd.write_stdin("1 + 1\n:quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("=> 2"));
}

#[test]
fn test_repl_variable_declaration() {
    // Test that variable declarations are stored (no output)
    let mut cmd = miri_cmd();
    cmd.write_stdin("let x = 10\n:quit\n").assert().success();
}

#[test]
fn test_repl_persistent_context() {
    // Test that context persists across lines
    let mut cmd = miri_cmd();
    cmd.write_stdin("let x = 10\nx + 1\n:quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("=> 11"));
}

#[test]
fn test_repl_mutable_assignment() {
    // Test that mutable variable assignments persist
    let mut cmd = miri_cmd();
    cmd.write_stdin("var y = 10\ny = 100\ny\n:quit\n")
        .assert()
        .success()
        .stdout(predicates::str::contains("=> 100"));
}
