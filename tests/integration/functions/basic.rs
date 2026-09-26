// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_simple_function() {
    assert_runs_with_output(
        r#"

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

fn square(x int) int
    x * x

fn main()
    println(f"{square(3) + square(4)}")
    "#,
        "25",
    );
}

#[test]
fn test_user_function_named_like_a_runtime_symbol() {
    assert_runs_with_output(
        r#"

fn miri_add(a int, b int) int
    return a + b

fn main()
    println(f"{miri_add(2, 3)}")
    "#,
        "5",
    );
}

#[test]
fn test_user_function_named_like_a_runtime_symbol_as_a_value() {
    assert_runs_with_output(
        r#"

fn miri_twice(x int) int
    return x * 2

fn apply(f fn(x int) int, x int) int
    return f(x)

fn main()
    println(f"{apply(miri_twice, 21)}")
    "#,
        "42",
    );
}
