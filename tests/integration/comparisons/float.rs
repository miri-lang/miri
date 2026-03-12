// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
