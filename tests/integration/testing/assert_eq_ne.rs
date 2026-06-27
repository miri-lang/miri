// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_assert_eq_int_pass_silent() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_eq(1 + 1, 2)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_int_fail_shows_values() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq(1, 2)
"#,
        "expected 2",
    );
}

#[test]
fn test_assert_eq_int_fail_shows_actual() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq(1, 2)
"#,
        "got 1",
    );
}

#[test]
fn test_assert_eq_int_fail_includes_source_location() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq(7, 8)
"#,
        ":5",
    );
}

#[test]
fn test_assert_eq_bool_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_eq(true, true)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_bool_fail() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq(true, false)
"#,
        "expected false",
    );
}

#[test]
fn test_assert_eq_string_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_eq("hi", "hi")
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_string_fail_shows_values() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq("abc", "xyz")
"#,
        "expected \"xyz\"",
    );
}

#[test]
fn test_assert_eq_float_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_eq(1.5, 1.5)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_with_user_message() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_eq(1, 2, "balance mismatch after deposit")
"#,
        "balance mismatch after deposit",
    );
}

#[test]
fn test_assert_ne_int_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_ne(1, 2)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_ne_int_fail() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_ne(5, 5)
"#,
        "values must differ",
    );
}

#[test]
fn test_assert_ne_int_fail_shows_value() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_ne(5, 5)
"#,
        "both were 5",
    );
}

#[test]
fn test_assert_ne_with_user_message() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_ne(1, 1, "values must not match")
"#,
        "values must not match",
    );
}

#[test]
fn test_assert_eq_i32_typed_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    let a i32 = 7
    let b i32 = 7
    assert_eq(a, b)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_u32_typed_fail_shows_values() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    let a u32 = 9
    let b u32 = 10
    assert_eq(a, b)
"#,
        "expected 10",
    );
}

#[test]
fn test_assert_eq_inside_user_function() {
    // Confirms allocator threading still works when the call site is inside
    // a non-main function where allocator is an explicit parameter.
    assert_runs_with_output(
        r#"
use system.testing

fn check(x int)
    assert_eq(x, 42)

fn main()
    check(42)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_eq_inside_user_function_fail() {
    assert_runtime_error(
        r#"
use system.testing

fn check(x int)
    assert_eq(x, 42)

fn main()
    check(7)
"#,
        "expected 42",
    );
}

#[test]
fn test_assert_eq_mismatched_argument_types_rejected() {
    // Confirms the standard arg type-checking applies to the intrinsic
    // generics, so callers cannot silently compare unrelated types.
    assert_compiler_error(
        r#"
use system.testing

fn main()
    assert_eq(1, "hello")
"#,
        "Type mismatch",
    );
}
