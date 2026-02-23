// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_operation_outputs, assert_runs_with_output};

#[test]
fn test_if_else_inline() {
    assert_operation_outputs(&[("if true: 1 else: 0", "1"), ("if false: 1 else: 0", "0")]);
}

#[test]
fn test_if_else_block() {
    assert_runs_with_output(
        r#"
use system.io

let x = 10
let y = if x > 5
    x * 2
else
    x
print(f"{y}")
        "#,
        "20",
    );
}

#[test]
fn test_if_else_if_else() {
    assert_runs_with_output(
        r#"
use system.io
let x = 5
let y = if x > 10
    100
else if x > 3
    50
else
    0
print(f"{y}")
    "#,
        "50",
    );
}

#[test]
fn test_unless_inline() {
    assert_operation_outputs(&[
        ("unless false: 1 else: 0", "1"),
        ("unless true: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_nested_if() {
    assert_runs_with_output(
        r#"
use system.io
let x = 15
let y = if x > 10
    if x > 20
        3
    else
        2
else
    1
print(f"{y}")
        "#,
        "2",
    );
}

#[test]
fn test_while_loop() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
var i = 0
while i < 5
    x = x + i
    i = i + 1
print(f"{x}")
    "#,
        "10",
    ); // 0+1+2+3+4 = 10
}

#[test]
fn test_for_loop_range() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
for i in 1..5
    sum = sum + i
print(f"{sum}")
    "#,
        "10",
    ); // 1+2+3+4 = 10
}

#[test]
fn test_break_in_while() {
    assert_runs_with_output(
        r#"
use system.io
var x = 0
while true
    x = x + 1
    if x >= 5
        break
print(f"{x}")
    "#,
        "5",
    );
}

#[test]
fn test_continue_in_for() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
for i in 1..10
    if i % 2 == 0
        continue
    sum = sum + i
print(f"{sum}")
    "#,
        "25",
    ); // 1+3+5+7+9 = 25
}

#[test]
fn test_comparison_operators() {
    assert_operation_outputs(&[
        ("if 5 > 3: 1 else: 0", "1"),
        ("if 5 < 3: 1 else: 0", "0"),
        ("if 5 >= 5: 1 else: 0", "1"),
        ("if 5 <= 5: 1 else: 0", "1"),
        ("if 5 == 5: 1 else: 0", "1"),
        ("if 5 != 5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_logical_and() {
    assert_operation_outputs(&[
        ("if true and true: 1 else: 0", "1"),
        ("if true and false: 1 else: 0", "0"),
        ("if false and true: 1 else: 0", "0"),
        ("if false and false: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_logical_or() {
    assert_operation_outputs(&[
        ("if true or true: 1 else: 0", "1"),
        ("if true or false: 1 else: 0", "1"),
        ("if false or true: 1 else: 0", "1"),
        ("if false or false: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_nested_loops() {
    assert_runs_with_output(
        r#"
use system.io
var sum = 0
for i in 1..4
    for j in 1..4
        sum = sum + 1
print(f"{sum}")
    "#,
        "9",
    ); // 3 * 3 = 9
}
