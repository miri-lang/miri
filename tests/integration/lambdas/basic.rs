// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Acceptance criterion: basic non-capturing lambda compiles and runs.
#[test]
fn test_lambda_basic() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let f = fn(x int) int: x * x
    println(f"{f(5)}")
    "#,
        "25",
    );
}

/// Lambda assigned to variable and called multiple times.
#[test]
fn test_lambda_called_multiple_times() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let double = fn(x int) int: x * 2
    println(f"{double(3)}")
    println(f"{double(7)}")
    "#,
        "6",
    );
}

/// Lambda with multiple parameters.
#[test]
fn test_lambda_multiple_params() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let add = fn(a int, b int) int: a + b
    println(f"{add(3, 4)}")
    "#,
        "7",
    );
}

/// Lambda passed as a function argument.
#[test]
fn test_lambda_passed_as_argument() {
    assert_runs_with_output(
        r#"
use system.io

fn apply(f fn(x int) int, n int) int
    f(n)

fn main()
    let square = fn(x int) int: x * x
    println(f"{apply(square, 6)}")
    "#,
        "36",
    );
}
