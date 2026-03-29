// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests that verify the type checker catches argument-count and type
//! mismatches when calling regular functions.

use super::utils::*;

// ── Too few arguments ─────────────────────────────────────────────────────────

#[test]
fn test_function_too_few_args() {
    assert_compiler_error(
        r#"
fn add(a int, b int) int
    a + b

fn main()
    let _ = add(1)
    "#,
        "Missing argument for parameter 'b'",
    );
}

#[test]
fn test_function_no_args_when_params_required() {
    assert_compiler_error(
        r#"
fn greet(name String)
    ()

fn main()
    greet()
    "#,
        "Missing argument for parameter 'name'",
    );
}

// ── Too many arguments ────────────────────────────────────────────────────────

#[test]
fn test_function_too_many_positional_args() {
    assert_compiler_error(
        r#"
fn double(x int) int
    x * 2

fn main()
    let _ = double(3, 4)
    "#,
        "Too many positional arguments",
    );
}

#[test]
fn test_function_args_to_no_param_function() {
    assert_compiler_error(
        r#"
fn noop()
    ()

fn main()
    noop(99)
    "#,
        "Too many positional arguments",
    );
}

// ── Wrong argument types ──────────────────────────────────────────────────────

#[test]
fn test_function_wrong_arg_type() {
    assert_compiler_error(
        r#"
fn square(n int) int
    n * n

fn main()
    let _ = square("hello")
    "#,
        "Type mismatch for argument 'n'",
    );
}

#[test]
fn test_function_wrong_second_arg_type() {
    assert_compiler_error(
        r#"
fn concat(a String, b String) String
    a

fn main()
    let _ = concat("hi", 42)
    "#,
        "Type mismatch for argument 'b'",
    );
}

// ── Unknown named argument ────────────────────────────────────────────────────

#[test]
fn test_function_unknown_named_arg() {
    assert_compiler_error(
        r#"
fn inc(n int) int
    n + 1

fn main()
    let _ = inc(x: 5)
    "#,
        "Unknown argument 'x'",
    );
}

// ── Valid calls must still compile ───────────────────────────────────────────

#[test]
fn test_function_correct_call() {
    assert_type_checks(
        r#"
use system.io

fn add(a int, b int) int
    a + b

fn main()
    println(f"{add(3, 4)}")
    "#,
    );
}

#[test]
fn test_function_named_args_reordered() {
    assert_type_checks(
        r#"
use system.io

fn sub(a int, b int) int
    a - b

fn main()
    println(f"{sub(b: 2, a: 10)}")
    "#,
    );
}

#[test]
fn test_function_default_param_can_be_omitted() {
    assert_type_checks(
        r#"
use system.io

fn greet(name String, prefix String = "Hello") String
    prefix

fn main()
    let _ = greet("Alice")
    println("ok")
    "#,
    );
}
