// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_param_single_variant() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn status_code(s Status) int
    match s
        Status.Ok: 0
        Status.Error: 1

fn main()
    let r = status_code(Status.Ok)
    println(f"{r}")
"#,
        "0",
    );
}

#[test]
fn test_enum_param_error_variant() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn status_code(s Status) int
    match s
        Status.Ok: 0
        Status.Error: 1

fn main()
    let r = status_code(Status.Error)
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_enum_param_three_variants() {
    assert_runs_with_output(
        r#"
use system.io

enum Color
    Red
    Green
    Blue

fn color_index(c Color) int
    match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3

fn main()
    println(f"{color_index(Color.Red)}")
    println(f"{color_index(Color.Green)}")
    println(f"{color_index(Color.Blue)}")
"#,
        "1\n2\n3", // Original was "2" but it print 1, 2, 3
    );
}

#[test]
fn test_enum_return_from_function() {
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
    let t = flip(Toggle.On)
    let r = match t
        Toggle.On: 1
        Toggle.Off: 0
    println(f"{r}")
"#,
        "0",
    );
}
