// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, interpreter_assert_returns};

#[test]
fn test_simple_enum() {
    assert_runs(
        r#"
enum Status
    Ok
    Error

fn main()
    let s = Status.Ok
    "#,
    );
}

#[test]
fn test_enum_with_data() {
    assert_runs(
        r#"
enum Result
    Success(int)
    Failure(string)

fn main()
    let r = Result.Success(42)
    "#,
    );
}

#[test]
fn test_enum_match() {
    interpreter_assert_returns(
        r#"
enum Status
    Ok
    Error

fn main() int
    let s = Status.Ok
    match s
        Status.Ok: 1
        Status.Error: 0
    "#,
        1,
    );
}

#[test]
fn test_enum_match_multiple_variants() {
    interpreter_assert_returns(
        r#"
enum Color
    Red
    Green
    Blue

fn main() int
    let c = Color.Green
    match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3
    "#,
        2,
    );
}
