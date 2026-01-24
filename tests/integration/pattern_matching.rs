// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_returns, interpreter_assert_returns};

#[test]
fn test_match_integer_literals() {
    assert_returns(
        r#"
let val = 1
match val
    1: 100
    2: 200
    _: 0
        "#,
        100,
    );
}

#[test]
fn test_match_variable() {
    assert_returns(
        r#"
let x = 42
match x
    0: 0
    42: 1
    _: 2
    "#,
        1,
    );
}

#[test]
fn test_match_with_guards() {
    interpreter_assert_returns(
        r#"
let x = 15
match x
    n if n > 10: 1
    n if n > 5: 2
    _: 3
    "#,
        1,
    );
}

#[test]
fn test_match_identifier_binding() {
    interpreter_assert_returns(
        r#"
let x = 42
match x
    value: value * 2
    "#,
        84,
    );
}

#[test]
fn test_match_default() {
    assert_returns(
        r#"
let val = 99
match val
    1: 10
    2: 20
    _: 999
    "#,
        999,
    );
}

#[test]
fn test_match_nested() {
    assert_returns(
        r#"
let outer = 1
let inner = 2
match outer
    1: match inner
        1: 10
        2: 20
        _: 30
    _: 0
    "#,
        20,
    );
}
