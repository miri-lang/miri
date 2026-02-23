// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

#[test]
fn test_simple_function() {
    assert_runs_with_output(
        r#"
use system.io

fn add(a int, b int) int
    a + b

fn main()
    println(f"{add(3, 4)}")
    "#,
        "7",
    );
}

#[test]
fn test_function_no_args() {
    assert_runs_with_output(
        r#"
use system.io

fn answer() int
    42

fn main()
    println(f"{answer()}")
    "#,
        "42",
    );
}

#[test]
fn test_function_single_arg() {
    assert_runs_with_output(
        r#"
use system.io

fn double(x int) int
    x * 2

fn main()
    println(f"{double(21)}")
    "#,
        "42",
    );
}

#[test]
fn test_function_multiple_calls() {
    assert_runs_with_output(
        r#"
use system.io

fn square(x int) int
    x * x

fn main()
    println(f"{square(3) + square(4)}")
    "#,
        "25",
    );
}

#[test]
fn test_recursive_factorial() {
    assert_runs_with_output(
        r#"
use system.io

fn factorial(n int) int
    if n <= 1
        1
    else
        n * factorial(n - 1)

fn main()
    println(f"{factorial(5)}")
    "#,
        "120",
    );
}

#[test]
fn test_recursive_fibonacci() {
    assert_runs_with_output(
        r#"
use system.io

fn fib(n int) int
    if n <= 1
        n
    else
        fib(n - 1) + fib(n - 2)

fn main()
    println(f"{fib(10)}")
    "#,
        "55",
    );
}

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
    ); // (2*3) + (4*5) = 6 + 20 = 26
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
    ); // 20 + 30 = 50
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
