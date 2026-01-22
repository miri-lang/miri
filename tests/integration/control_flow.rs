// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{
    assert_returns, assert_returns_many, interpreter_assert_returns_many,
};

#[test]
fn test_if_else_inline() {
    assert_returns_many(&[("if true: 1 else: 0", 1), ("if false: 1 else: 0", 0)]);
}

#[test]
fn test_if_else_block() {
    assert_returns(
        r#"
let x = 10
if x > 5
    x * 2
else
    x
        "#,
        20,
    );
}

#[test]
fn test_if_else_if_else() {
    assert_returns(
        r#"
let x = 5
if x > 10
    100
else if x > 3
    50
else
    0
    "#,
        50,
    );
}

#[test]
fn test_unless_inline() {
    assert_returns_many(&[
        ("unless false: 1 else: 0", 1),
        ("unless true: 1 else: 0", 0),
    ]);
}

#[test]
fn test_nested_if() {
    assert_returns(
        r#"
let x = 15
if x > 10
    if x > 20
        3
    else
        2
else
    1
        "#,
        2,
    );
}

#[test]
fn test_while_loop() {
    assert_returns(
        r#"
var x = 0
var i = 0
while i < 5
    x = x + i
    i = i + 1
x
    "#,
        10,
    ); // 0+1+2+3+4 = 10
}

#[test]
fn test_for_loop_range() {
    assert_returns(
        r#"
var sum = 0
for i in 1..5
    sum = sum + i
sum
    "#,
        10,
    ); // 1+2+3+4 = 10
}

#[test]
fn test_break_in_while() {
    assert_returns(
        r#"
var x = 0
while true
    x = x + 1
    if x >= 5
        break
x
    "#,
        5,
    );
}

#[test]
fn test_continue_in_for() {
    assert_returns(
        r#"
var sum = 0
for i in 1..10
    if i % 2 == 0
        continue
    sum = sum + i
sum
    "#,
        25,
    ); // 1+3+5+7+9 = 25
}

#[test]
fn test_comparison_operators() {
    assert_returns_many(&[
        ("if 5 > 3: 1 else: 0", 1),
        ("if 5 < 3: 1 else: 0", 0),
        ("if 5 >= 5: 1 else: 0", 1),
        ("if 5 <= 5: 1 else: 0", 1),
        ("if 5 == 5: 1 else: 0", 1),
        ("if 5 != 5: 1 else: 0", 0),
    ]);
}

#[test]
fn test_logical_and() {
    // TODO: Switch to assert_returns_many when codegen supports short-circuit and
    interpreter_assert_returns_many(&[
        ("if true and true: 1 else: 0", 1),
        ("if true and false: 1 else: 0", 0),
        ("if false and true: 1 else: 0", 0),
        ("if false and false: 1 else: 0", 0),
    ]);
}

#[test]
fn test_logical_or() {
    // TODO: Switch to assert_returns_many when codegen supports short-circuit or
    interpreter_assert_returns_many(&[
        ("if true or true: 1 else: 0", 1),
        ("if true or false: 1 else: 0", 1),
        ("if false or true: 1 else: 0", 1),
        ("if false or false: 1 else: 0", 0),
    ]);
}

#[test]
fn test_nested_loops() {
    assert_returns(
        r#"
var sum = 0
for i in 1..4
    for j in 1..4
        sum = sum + 1
sum
    "#,
        9,
    ); // 3 * 3 = 9
}
