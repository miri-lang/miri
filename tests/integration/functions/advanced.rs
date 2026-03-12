// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_nested_function_calls() {
    assert_runs_with_output(
        r#"
use system.io

fn add(a int, b int) int
    a + b

fn mul(a int, b int) int
    a * b

fn main()
    println(f"{add(mul(2, 3), mul(4, 5))}")
    "#,
        "26",
    );
}

#[test]
fn test_function_with_local_vars() {
    assert_runs_with_output(
        r#"
use system.io

fn compute(x int) int
    let doubled = x * 2
    let tripled = x * 3
    doubled + tripled

fn main()
    println(f"{compute(10)}")
    "#,
        "50",
    );
}

#[test]
fn test_function_guards() {
    assert_runs(
        r#"
fn positive_only(x int > 0) int
    x * 2

fn main()
    positive_only(5)
    "#,
    );
}

#[test]
fn test_function_with_conditional() {
    assert_runs_with_output(
        r#"
use system.io

fn abs(x int) int
    if x < 0
        -x
    else
        x

fn main()
    println(f"{abs(-42)}")
    "#,
        "42",
    );
}
