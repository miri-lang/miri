// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_unless_inline() {
    assert_operation_outputs(&[
        ("unless false: 1 else: 0", "1"),
        ("unless true: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_unless_block() {
    assert_runs_with_output(
        r#"
use system.io
var x = 3
unless x > 10
    x = x + 1
print(f"{x}")
    "#,
        "4",
    );
}

#[test]
fn test_unless_block_condition_true() {
    assert_runs_with_output(
        r#"
use system.io
var x = 20
unless x > 10
    x = 0
print(f"{x}")
    "#,
        "20", // condition is true → body skipped
    );
}

#[test]
fn test_unless_block_with_else() {
    assert_runs_with_output(
        r#"
use system.io
var x = 15
unless x < 10
    x = 99
else
    x = 0
print(f"{x}")
    "#,
        "99", // condition is false → unless body runs
    );
}
