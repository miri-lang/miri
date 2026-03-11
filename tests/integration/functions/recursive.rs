// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
