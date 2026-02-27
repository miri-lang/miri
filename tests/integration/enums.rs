// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

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

// =============================================================================
// Enum match with wildcard fallback
// =============================================================================

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

// =============================================================================
// Enum match inside a function
// =============================================================================

#[test]
fn test_enum_match_in_function() {
    assert_runs_with_output(
        r#"
use system.io

enum Direction
    North
    South
    East
    West

fn direction_code(d Direction) int
    match d
        Direction.North: 0
        Direction.South: 1
        Direction.East: 2
        Direction.West: 3

fn main()
    println(f"{direction_code(Direction.North)}")
    println(f"{direction_code(Direction.West)}")
        "#,
        "0",
    );
}

// =============================================================================
// Enum as function parameter matched inside
// =============================================================================

#[test]
fn test_enum_param_match_all_variants() {
    assert_runs_with_output(
        r#"
use system.io

enum Signal
    Red
    Yellow
    Green

fn signal_wait(s Signal) int
    match s
        Signal.Red: 3
        Signal.Yellow: 1
        Signal.Green: 0

fn main()
    let r = signal_wait(Signal.Red)
    let y = signal_wait(Signal.Yellow)
    let g = signal_wait(Signal.Green)
    println(f"{r}")
    println(f"{y}")
    println(f"{g}")
        "#,
        "3",
    );
}

// =============================================================================
// Enum match returning enum value
// =============================================================================

#[test]
fn test_enum_match_returns_enum() {
    assert_runs_with_output(
        r#"
use system.io

enum Toggle
    On
    Off

fn flip(t Toggle) Toggle
    match t
        Toggle.On: Toggle.Off
        Toggle.Off: Toggle.On

fn main()
    let t1 = flip(Toggle.On)
    let t2 = flip(Toggle.Off)
    let r1 = match t1
        Toggle.On: 1
        Toggle.Off: 0
    let r2 = match t2
        Toggle.On: 1
        Toggle.Off: 0
    println(f"{r1}")
    println(f"{r2}")
        "#,
        "0",
    );
}

// =============================================================================
// Enum match with guard
// =============================================================================

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

// =============================================================================
// Non-exhaustive enum match (type checker error)
// =============================================================================

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

// =============================================================================
// Enum with data: extract associated value
// =============================================================================

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

// =============================================================================
// Regression test: EnumValue DPS support
// =============================================================================

/// Regression test: EnumValue (Option::Some syntax) must support DPS.
/// Previously, EnumValue always allocated a fresh temp and ignored the
/// caller-provided destination, leaving the destination uninitialized.
#[test]
fn test_enum_value_dps_in_match_result() {
    assert_runs_with_output(
        r#"
use system.io

enum Option: Some(int), None

fn make_option(x int) Option
    Option.Some(x)

fn main()
    let opt = make_option(42)
    let result = match opt
        Option.None: 0
        Option.Some(v): v
    print(f"{result}")
        "#,
        "42",
    );
}

/// Regression test: enum variant constructor via Call path must work when used
/// as a variable initializer (DPS passes dest to the lowering).
#[test]
fn test_enum_variant_constructor_call_dps() {
    assert_runs_with_output(
        r#"
use system.io

enum Result: Ok(int), Err(int)

fn main()
    let r = Result.Ok(100)
    let val = match r
        Result.Ok(v): v
        Result.Err(e): e
    print(f"{val}")
        "#,
        "100",
    );
}
