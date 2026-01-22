// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_returns, interpreter_assert_runs};

#[test]
fn test_simple_function() {
    assert_returns(
        r#"
fn add(a int, b int) int
    a + b

fn main() int
    add(3, 4)
    "#,
        7,
    );
}

#[test]
fn test_function_no_args() {
    assert_returns(
        r#"
fn answer() int
    42

fn main() int
    answer()
    "#,
        42,
    );
}

#[test]
fn test_function_single_arg() {
    assert_returns(
        r#"
fn double(x int) int
    x * 2

fn main() int
    double(21)
    "#,
        42,
    );
}

#[test]
fn test_function_multiple_calls() {
    assert_returns(
        r#"
fn square(x int) int
    x * x

fn main() int
    square(3) + square(4)
    "#,
        25,
    );
}

#[test]
fn test_recursive_factorial() {
    assert_returns(
        r#"
fn factorial(n int) int
    if n <= 1
        1
    else
        n * factorial(n - 1)

fn main() int
    factorial(5)
    "#,
        120,
    );
}

#[test]
fn test_recursive_fibonacci() {
    assert_returns(
        r#"
fn fib(n int) int
    if n <= 1
        n
    else
        fib(n - 1) + fib(n - 2)

fn main() int
    fib(10)
    "#,
        55,
    );
}

#[test]
fn test_nested_function_calls() {
    assert_returns(
        r#"
fn add(a int, b int) int
    a + b

fn mul(a int, b int) int
    a * b

fn main() int
    add(mul(2, 3), mul(4, 5))
    "#,
        26,
    ); // (2*3) + (4*5) = 6 + 20 = 26
}

#[test]
fn test_function_with_local_vars() {
    assert_returns(
        r#"
fn compute(x int) int
    let doubled = x * 2
    let tripled = x * 3
    doubled + tripled

fn main() int
    compute(10)
    "#,
        50,
    ); // 20 + 30 = 50
}

#[test]
fn test_function_guards() {
    // TODO: Switch to assert_runs when codegen supports function guards
    interpreter_assert_runs(
        r#"
fn positive_only(x int > 0) int
    x * 2

fn main() int
    positive_only(5)
    "#,
    );
}

#[test]
fn test_function_with_conditional() {
    assert_returns(
        r#"
fn abs(x int) int
    if x < 0
        -x
    else
        x

fn main() int
    abs(-42)
    "#,
        42,
    );
}
