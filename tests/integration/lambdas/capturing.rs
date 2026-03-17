// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Acceptance criterion: integer capture.
#[test]
fn test_capture_int() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var x = 10
    let f = fn() int: x + 1
    println(f"{f()}")
    "#,
        "11",
    );
}

/// Capture and use in expression with lambda param.
#[test]
fn test_capture_with_param() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let base = 10
    let add = fn(n int) int: base + n
    println(f"{add(5)}")
    "#,
        "15",
    );
}

/// Capture multiple variables.
#[test]
fn test_capture_multiple() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a = 3
    let b = 4
    let sum = fn() int: a + b
    println(f"{sum()}")
    "#,
        "7",
    );
}

/// Capture a string (pointer capture).
#[test]
fn test_capture_string() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let greeting = "Hello"
    let f = fn() String: greeting
    println(f())
    "#,
        "Hello",
    );
}
