// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
    // Use a value ≤ 127 to stay in the signed-safe range
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
