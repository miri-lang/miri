// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs_with_output;

#[test]
fn test_match_integer_literals() {
    assert_runs_with_output(
        r#"
use system.io

let val = 1
let result = match val
    1: 100
    2: 200
    _: 0
print(result)
        "#,
        "100",
    );
}

#[test]
fn test_match_variable() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
let result = match x
    0: 0
    42: 1
    _: 2
print(result)
    "#,
        "1",
    );
}

#[test]
fn test_match_with_guards() {
    assert_runs_with_output(
        r#"
use system.io

let x = 15
let result = match x
    n if n > 10: 1
    n if n > 5: 2
    _: 3
print(result)
    "#,
        "1",
    );
}

#[test]
fn test_match_identifier_binding() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
let result = match x
    value: value * 2
print(result)
    "#,
        "84",
    );
}

#[test]
fn test_match_default() {
    assert_runs_with_output(
        r#"
use system.io

let val = 99
let result = match val
    1: 10
    2: 20
    _: 999
print(result)
    "#,
        "999",
    );
}

#[test]
fn test_match_nested() {
    assert_runs_with_output(
        r#"
use system.io

let outer = 1
let inner = 2
let result = match outer
    1: match inner
        1: 10
        2: 20
        _: 30
    _: 0
print(result)
    "#,
        "20",
    );
}
