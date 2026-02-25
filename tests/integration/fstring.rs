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
// Typed integer width variables in f-strings
// =============================================================================

#[test]
fn test_fstring_i8_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i8(x i8)
    print(f"{x}")

show_i8(42)
    "#,
        "42",
    );
}

#[test]
fn test_fstring_i16_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i16(x i16)
    print(f"{x}")

show_i16(1000)
    "#,
        "1000",
    );
}

#[test]
fn test_fstring_i32_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i32(x i32)
    print(f"{x}")

show_i32(100000)
    "#,
        "100000",
    );
}

#[test]
fn test_fstring_i64_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i64(x i64)
    print(f"{x}")

show_i64(1000000000)
    "#,
        "1000000000",
    );
}

#[test]
fn test_fstring_u8_variable() {
    // Use a value ≤ 127 to stay in the signed-safe range (sextend is used for
    // integer widening, so values with the high bit set are not supported here).
    assert_runs_with_output(
        r#"
use system.io

fn show_u8(x u8)
    print(f"{x}")

show_u8(100)
    "#,
        "100",
    );
}

#[test]
fn test_fstring_negative_i8() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i8(x i8)
    print(f"{x}")

show_i8(-42)
    "#,
        "-42",
    );
}

#[test]
fn test_fstring_negative_i32() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i32(x i32)
    print(f"{x}")

show_i32(-100000)
    "#,
        "-100000",
    );
}

#[test]
fn test_fstring_zero_i8() {
    assert_runs_with_output(
        r#"
use system.io

fn show_i8(x i8)
    print(f"{x}")

show_i8(0)
    "#,
        "0",
    );
}

// =============================================================================
// Typed float width variables in f-strings
// =============================================================================

#[test]
fn test_fstring_f32_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_f32(x f32)
    print(f"{x}")

show_f32(1.5)
    "#,
        "1.5",
    );
}

#[test]
fn test_fstring_f64_variable() {
    assert_runs_with_output(
        r#"
use system.io

fn show_f64(x f64)
    print(f"{x}")

show_f64(3.14)
    "#,
        "3.14",
    );
}

// =============================================================================
// F-strings in function bodies (non-script context)
// =============================================================================

#[test]
fn test_fstring_in_function_return() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn greet(name string) string
    f"Hello, {name}!"

print(greet("Miri"))
    "#,
        "Hello, Miri!",
    );
}

#[test]
fn test_fstring_in_function_body_with_int() {
    assert_runs_with_output(
        r#"
use system.io

fn describe(n int) string
    f"value={n}"

print(describe(7))
    "#,
        "value=7",
    );
}

// =============================================================================
// Multiple references to the same variable
// =============================================================================

#[test]
fn test_fstring_same_variable_twice() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
print(f"{x} + {x} = {x + x}")
    "#,
        "5 + 5 = 10",
    );
}

// =============================================================================
// F-string as a function argument
// =============================================================================

#[test]
fn test_fstring_as_println_argument() {
    assert_runs_with_output(
        r#"
use system.io

let n = 42
println(f"answer={n}")
    "#,
        "answer=42",
    );
}

#[test]
fn test_fstring_as_string_param() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn show(s string)
    println(s)

let x = 99
show(f"x is {x}")
    "#,
        "x is 99",
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
