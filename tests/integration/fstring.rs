// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs_with_output};

// =============================================================================
// F-string interpolation with various types
// =============================================================================

#[test]
fn test_fstring_int() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
print(f"{x}")
    "#,
        "42",
    );
}

#[test]
fn test_fstring_float() {
    assert_runs_with_output(
        r#"
use system.io

let x = 3.14
print(f"{x}")
    "#,
        "3.14",
    );
}

#[test]
fn test_fstring_bool() {
    assert_runs_with_output(
        r#"
use system.io

let x = true
print(f"{x}")
    "#,
        "true",
    );
}

#[test]
fn test_fstring_mixed_types() {
    assert_runs_with_output(
        r#"
use system.io

let name = "Miri"
let version = 1
let active = true
print(f"{name} v{version} active={active}")
    "#,
        "Miri v1 active=true",
    );
}

#[test]
fn test_fstring_expression() {
    assert_runs_with_output(
        r#"
use system.io

print(f"{2 + 3 * 4}")
    "#,
        "14",
    );
}

#[test]
fn test_fstring_method_call() {
    assert_runs_with_output(
        r#"
use system.io

fn double(x int) int
    return x * 2

print(f"double(5) = {double(5)}")
    "#,
        "double(5) = 10",
    );
}

#[test]
fn test_fstring_string_method_call() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello"
print(f"{s.to_upper()}")
    "#,
        "HELLO",
    );
}

#[test]
fn test_fstring_nested_expressions() {
    assert_runs_with_output(
        r#"
use system.io

let a = 10
let b = 20
print(f"{a} + {b} = {a + b}")
    "#,
        "10 + 20 = 30",
    );
}

#[test]
fn test_fstring_empty() {
    assert_runs_with_output(
        r#"
use system.io

print(f"")
    "#,
        "",
    );
}

#[test]
fn test_fstring_no_interpolation() {
    assert_runs_with_output(
        r#"
use system.io

print(f"just a plain string")
    "#,
        "just a plain string",
    );
}

// =============================================================================
// println type errors
// =============================================================================

#[test]
fn test_println_int_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(42)
    "#,
        "Type mismatch",
    );
}

#[test]
fn test_println_bool_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(true)
    "#,
        "Type mismatch",
    );
}

#[test]
fn test_println_float_type_error() {
    assert_compiler_error(
        r#"
use system.io

println(3.14)
    "#,
        "Type mismatch",
    );
}
