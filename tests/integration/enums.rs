// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

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
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn main()
    let s = Status.Ok
    let result = match s
        Status.Ok: 1
        Status.Error: 0
    print(result)
    "#,
        "1",
    );
}

#[test]
fn test_enum_match_multiple_variants() {
    assert_runs_with_output(
        r#"
use system.io

enum Color
    Red
    Green
    Blue

fn main()
    let c = Color.Green
    let result = match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3
    print(result)
    "#,
        "2",
    );
}
