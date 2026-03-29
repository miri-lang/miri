// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests that verify the type checker catches argument-count and type
//! mismatches when calling lambda expressions.

use crate::integration::utils::*;

// ── Too few arguments ─────────────────────────────────────────────────────────

#[test]
fn test_lambda_too_few_args() {
    assert_compiler_error(
        r#"
fn main()
    let add = fn(a int, b int) int: a + b
    let _ = add(1)
    "#,
        "Missing argument for parameter 'b'",
    );
}

// ── Too many arguments ────────────────────────────────────────────────────────

#[test]
fn test_lambda_too_many_args() {
    assert_compiler_error(
        r#"
fn main()
    let double = fn(x int) int: x * 2
    let _ = double(3, 4)
    "#,
        "Too many positional arguments",
    );
}

#[test]
fn test_lambda_args_to_no_param_lambda() {
    assert_compiler_error(
        r#"
fn main()
    let noop = fn() int: 42
    let _ = noop(99)
    "#,
        "Too many positional arguments",
    );
}

// ── Wrong argument types ──────────────────────────────────────────────────────

#[test]
fn test_lambda_wrong_arg_type() {
    assert_compiler_error(
        r#"
fn main()
    let inc = fn(n int) int: n + 1
    let _ = inc("hello")
    "#,
        "Type mismatch for argument 'n'",
    );
}

// ── Valid calls must still compile ───────────────────────────────────────────

#[test]
fn test_lambda_correct_call() {
    assert_type_checks(
        r#"
use system.io

fn main()
    let add = fn(a int, b int) int: a + b
    println(f"{add(3, 4)}")
    "#,
    );
}

#[test]
fn test_lambda_passed_to_function_correct_arity() {
    // Lambda passed as a higher-order function argument — arity must still check.
    assert_type_checks(
        r#"
use system.io

fn apply(f fn(x int) int, n int) int
    f(n)

fn main()
    let result = apply(fn(n int) int: n * 2, 5)
    println(f"{result}")
    "#,
    );
}
