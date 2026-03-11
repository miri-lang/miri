// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
    print(f"{result}")
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
    print(f"{result}")
    "#,
        "2",
    );
}

#[test]
fn test_enum_match_with_wildcard() {
    assert_runs_with_output(
        r#"
use system.io

enum Direction
    North
    South
    East
    West

fn main()
    let d = Direction.East
    let result = match d
        Direction.North: 1
        Direction.South: 2
        _: 99
    print(f"{result}")
        "#,
        "99",
    );
}

#[test]
fn test_enum_match_with_guard() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn main()
    let s = Status.Ok
    let extra = 5
    let result = match s
        Status.Ok if extra > 3: 100
        Status.Ok: 50
        Status.Error: 0
    print(f"{result}")
        "#,
        "100",
    );
}

#[test]
fn test_enum_non_exhaustive_error() {
    assert_compiler_error(
        r#"
enum Color
    Red
    Green
    Blue

fn main()
    let c = Color.Red
    let result = match c
        Color.Red: 1
        Color.Green: 2
        "#,
        "Non-exhaustive",
    );
}

#[test]
fn test_enum_data_extraction() {
    assert_runs_with_output(
        r#"
use system.io

enum Wrapper
    Value(int)
    Empty

fn main()
    let w = Wrapper.Value(42)
    let result = match w
        Wrapper.Value(n): n
        Wrapper.Empty: 0
    print(f"{result}")
    "#,
        "42",
    );
}

#[test]
fn test_enum_data_extraction_empty_arm() {
    assert_runs_with_output(
        r#"
use system.io

enum Wrapper
    Value(int)
    Empty

fn main()
    let w = Wrapper.Empty
    let result = match w
        Wrapper.Value(n): n
        Wrapper.Empty: 0
    print(f"{result}")
    "#,
        "0",
    );
}
