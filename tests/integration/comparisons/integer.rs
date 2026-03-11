// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
