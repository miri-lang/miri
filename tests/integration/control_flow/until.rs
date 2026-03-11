// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_until_loop() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
var i = 0
until i >= 5
    x = x + i
    i = i + 1
print(f"{x}")
    "#,
        "10", // 0+1+2+3+4 = 10
    );
}

#[test]
fn test_until_loop_never_enters() {
    assert_runs_with_output(
        r#"
use system.io
var x = 42
until true
    x = 0
print(f"{x}")
    "#,
        "42", // condition true from start → body never runs
    );
}
