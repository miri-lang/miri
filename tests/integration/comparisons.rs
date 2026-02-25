// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Integration tests for comparison and logical operators across all supported types.
//!
//! Covers `==`, `!=`, `<`, `>`, `<=`, `>=`, `and`, `or`, `not` for:
//! - Default integer (int / i64)
//! - Typed signed integers (i8, i16, i32, i64)
//! - Floating-point (f32, f64)
//! - Boolean operands
//! - String equality (`==`) and inequality (`!=`)
//! Combined and edge-case patterns: negation of comparison results,
//! short-circuit evaluation, comparison results stored in variables.

use crate::integration::utils::{assert_operation_outputs, assert_runs_with_output};

// =============================================================================
// Float comparison operators (default float / f32 literals)
// =============================================================================

#[test]
fn test_float_eq() {
    assert_operation_outputs(&[
        ("if 1.5 == 1.5: 1 else: 0", "1"),
        ("if 1.5 == 2.5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_float_ne() {
    assert_operation_outputs(&[
        ("if 1.5 != 2.5: 1 else: 0", "1"),
        ("if 1.5 != 1.5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_float_lt() {
    assert_operation_outputs(&[
        ("if 1.5 < 2.5: 1 else: 0", "1"),
        ("if 2.5 < 1.5: 1 else: 0", "0"),
        ("if 1.5 < 1.5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_float_le() {
    assert_operation_outputs(&[
        ("if 1.5 <= 2.5: 1 else: 0", "1"),
        ("if 1.5 <= 1.5: 1 else: 0", "1"),
        ("if 2.5 <= 1.5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_float_gt() {
    assert_operation_outputs(&[
        ("if 2.5 > 1.5: 1 else: 0", "1"),
        ("if 1.5 > 2.5: 1 else: 0", "0"),
        ("if 1.5 > 1.5: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_float_ge() {
    assert_operation_outputs(&[
        ("if 2.5 >= 1.5: 1 else: 0", "1"),
        ("if 1.5 >= 1.5: 1 else: 0", "1"),
        ("if 1.5 >= 2.5: 1 else: 0", "0"),
    ]);
}

// =============================================================================
// f64-typed comparisons via function parameters
// =============================================================================

#[test]
fn test_f64_eq() {
    assert_runs_with_output(
        r#"
use system.io

fn cmp_f64(a f64, b f64) int
    if a == b
        1
    else
        0

fn main()
    println(f"{cmp_f64(3.0, 3.0)}")
    println(f"{cmp_f64(3.0, 4.0)}")
"#,
        "1",
    );
}

#[test]
fn test_f64_lt() {
    assert_runs_with_output(
        r#"
use system.io

fn lt_f64(a f64, b f64) int
    if a < b
        1
    else
        0

fn main()
    println(f"{lt_f64(1.0, 2.0)}")
    println(f"{lt_f64(2.0, 1.0)}")
"#,
        "1",
    );
}

#[test]
fn test_f64_gt() {
    assert_runs_with_output(
        r#"
use system.io

fn gt_f64(a f64, b f64) int
    if a > b
        1
    else
        0

fn main()
    println(f"{gt_f64(5.0, 3.0)}")
    println(f"{gt_f64(3.0, 5.0)}")
"#,
        "1",
    );
}

#[test]
fn test_f64_le_boundary() {
    assert_runs_with_output(
        r#"
use system.io

fn le_f64(a f64, b f64) int
    if a <= b
        1
    else
        0

fn main()
    println(f"{le_f64(2.0, 2.0)}")
    println(f"{le_f64(2.0, 3.0)}")
    println(f"{le_f64(3.0, 2.0)}")
"#,
        "1",
    );
}

#[test]
fn test_f64_ge_boundary() {
    assert_runs_with_output(
        r#"
use system.io

fn ge_f64(a f64, b f64) int
    if a >= b
        1
    else
        0

fn main()
    println(f"{ge_f64(2.0, 2.0)}")
    println(f"{ge_f64(3.0, 2.0)}")
    println(f"{ge_f64(2.0, 3.0)}")
"#,
        "1",
    );
}

// =============================================================================
// f32-typed comparisons via function parameters
// =============================================================================

#[test]
fn test_f32_all_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn eq_f32(a f32, b f32) int
    if a == b
        1
    else
        0

fn lt_f32(a f32, b f32) int
    if a < b
        1
    else
        0

fn gt_f32(a f32, b f32) int
    if a > b
        1
    else
        0

fn main()
    println(f"{eq_f32(1.5, 1.5)}")
    println(f"{eq_f32(1.5, 2.5)}")
    println(f"{lt_f32(1.0, 2.0)}")
    println(f"{gt_f32(3.0, 1.0)}")
"#,
        "1",
    );
}

// =============================================================================
// Typed signed integer comparisons via function parameters
// =============================================================================

#[test]
fn test_i8_all_comparisons() {
    // For a=10, b=20: ne=1, lt=1, le=1; eq=0, gt=0, ge=0 → sum = 3
    assert_runs_with_output(
        r#"
use system.io

fn cmp_i8(a i8, b i8) int
    let eq = if a == b: 1 else: 0
    let ne = if a != b: 1 else: 0
    let lt = if a < b: 1 else: 0
    let le = if a <= b: 1 else: 0
    let gt = if a > b: 1 else: 0
    let ge = if a >= b: 1 else: 0
    eq + ne + lt + le + gt + ge

fn main()
    println(f"{cmp_i8(10, 20)}")
"#,
        "3",
    );
}

#[test]
fn test_i16_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn eq_i16(a i16, b i16) int
    if a == b
        1
    else
        0

fn lt_i16(a i16, b i16) int
    if a < b
        1
    else
        0

fn gt_i16(a i16, b i16) int
    if a > b
        1
    else
        0

fn main()
    println(f"{eq_i16(1000, 1000)}")
    println(f"{eq_i16(1000, 2000)}")
    println(f"{lt_i16(500, 1000)}")
    println(f"{gt_i16(2000, 1000)}")
"#,
        "1",
    );
}

#[test]
fn test_i32_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn cmp_i32(a i32, b i32) int
    if a < b
        1
    else if a > b
        2
    else
        0

fn main()
    println(f"{cmp_i32(100000, 200000)}")
    println(f"{cmp_i32(200000, 100000)}")
    println(f"{cmp_i32(100000, 100000)}")
"#,
        "1",
    );
}

#[test]
fn test_i64_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn le_i64(a i64, b i64) int
    if a <= b
        1
    else
        0

fn ge_i64(a i64, b i64) int
    if a >= b
        1
    else
        0

fn main()
    println(f"{le_i64(1000000000, 1000000000)}")
    println(f"{le_i64(500000000, 1000000000)}")
    println(f"{ge_i64(1000000000, 500000000)}")
"#,
        "1",
    );
}

#[test]
fn test_i8_negative_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn lt_i8(a i8, b i8) int
    if a < b
        1
    else
        0

fn main()
    println(f"{lt_i8(-10, 10)}")
    println(f"{lt_i8(10, -10)}")
    println(f"{lt_i8(-10, -5)}")
"#,
        "1",
    );
}

#[test]
fn test_i8_boundary_comparisons() {
    assert_runs_with_output(
        r#"
use system.io

fn eq_i8(a i8, b i8) int
    if a == b
        1
    else
        0

fn main()
    println(f"{eq_i8(127, 127)}")
    println(f"{eq_i8(-128, -128)}")
    println(f"{eq_i8(127, -128)}")
"#,
        "1",
    );
}

// =============================================================================
// Combined logical and comparison operators
// =============================================================================

#[test]
fn test_and_with_comparisons() {
    assert_operation_outputs(&[
        ("if 5 > 3 and 10 < 20: 1 else: 0", "1"),
        ("if 5 > 3 and 10 > 20: 1 else: 0", "0"),
        ("if 5 < 3 and 10 < 20: 1 else: 0", "0"),
        ("if 5 < 3 and 10 > 20: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_or_with_comparisons() {
    assert_operation_outputs(&[
        ("if 5 > 3 or 10 > 20: 1 else: 0", "1"),
        ("if 5 < 3 or 10 < 20: 1 else: 0", "1"),
        ("if 5 < 3 or 10 > 20: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_not_comparison_result() {
    assert_operation_outputs(&[
        ("if not (5 == 3): 1 else: 0", "1"),
        ("if not (5 == 5): 1 else: 0", "0"),
        ("if not (5 > 3): 1 else: 0", "0"),
        ("if not (3 > 5): 1 else: 0", "1"),
    ]);
}

#[test]
fn test_double_not() {
    assert_operation_outputs(&[
        ("if not not true: 1 else: 0", "1"),
        ("if not not false: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_not_with_and_or() {
    assert_operation_outputs(&[
        ("if not (true and false): 1 else: 0", "1"),
        ("if not (true and true): 1 else: 0", "0"),
        ("if not (false or false): 1 else: 0", "1"),
        ("if not (false or true): 1 else: 0", "0"),
    ]);
}

// =============================================================================
// Short-circuit evaluation
//
// If `and` truly short-circuits, the right-hand side is NOT evaluated when
// the left-hand side is false. Using integer division by zero on the RHS
// verifies this: if RHS were evaluated it would trap and the test would fail.
// =============================================================================

#[test]
fn test_and_short_circuits_on_false_lhs() {
    assert_runs_with_output(
        r#"
use system.io

var x = 0
let result = if false and (1 / x == 0)
    1
else
    0
println(f"{result}")
"#,
        "0",
    );
}

#[test]
fn test_or_short_circuits_on_true_lhs() {
    assert_runs_with_output(
        r#"
use system.io

var x = 0
let result = if true or (1 / x == 0)
    1
else
    0
println(f"{result}")
"#,
        "1",
    );
}

// =============================================================================
// Comparison result stored in a variable
// =============================================================================

#[test]
fn test_comparison_result_in_bool_var() {
    assert_runs_with_output(
        r#"
use system.io

let a = 5
let b = 10
let lt = a < b
let gt = a > b
let eq = a == b
let r1 = if lt: 1 else: 0
let r2 = if gt: 1 else: 0
let r3 = if eq: 1 else: 0
println(f"{r1}")
println(f"{r2}")
println(f"{r3}")
"#,
        "1",
    );
}

#[test]
fn test_bool_var_chained_logic() {
    assert_runs_with_output(
        r#"
use system.io

let x = 7
let in_range = x > 0 and x < 10
let out_range = x < 0 or x > 10
let r1 = if in_range: 1 else: 0
let r2 = if out_range: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}

// =============================================================================
// Negative zero and equality
// =============================================================================

#[test]
fn test_int_zero_comparisons() {
    assert_operation_outputs(&[
        ("if 0 == 0: 1 else: 0", "1"),
        ("if 0 < 1: 1 else: 0", "1"),
        ("if 0 > -1: 1 else: 0", "1"),
        ("if -1 < 0: 1 else: 0", "1"),
        ("if 0 != 1: 1 else: 0", "1"),
    ]);
}

// =============================================================================
// String equality and inequality operators
// =============================================================================

#[test]
fn test_string_eq() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = "hello"
let c = "world"
let r1 = if a == b: 1 else: 0
let r2 = if a == c: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}

#[test]
fn test_string_ne() {
    assert_runs_with_output(
        r#"
use system.io

let a = "foo"
let b = "bar"
let c = "foo"
let r1 = if a != b: 1 else: 0
let r2 = if a != c: 1 else: 0
println(f"{r1}")
println(f"{r2}")
"#,
        "1",
    );
}
