// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
