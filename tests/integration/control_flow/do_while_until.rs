// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_do_while_basic() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
while x < 5
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_do_while_executes_once() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
while false
print(f"{x}")
    "#,
        "1", // body runs once even though condition is immediately false
    );
}

#[test]
fn test_do_until_basic() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
until x >= 5
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_do_until_executes_once() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
do
    x = x + 1
until true
print(f"{x}")
    "#,
        "1", // body runs once before the condition is checked
    );
}
