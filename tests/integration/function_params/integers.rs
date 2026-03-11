// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_i8_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i8(x i8) i8
    x * 2

fn main()
    let r = double_i8(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i16_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i16(x i16) i16
    x * 2

fn main()
    let r = double_i16(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i32(x i32) i32
    x * 2

fn main()
    let r = double_i32(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i64(x i64) i64
    x * 2

fn main()
    let r = double_i64(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i8_boundary_values() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_i8(x i8) i8
    x

fn main()
    let max = identity_i8(127)
    let min = identity_i8(-128)
    let ok = if max == 127
        if min == -128
            1
        else
            0
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i8_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_i8(a i8, b i8) i8
    a + b

fn main()
    let r = add_i8(10, 32)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i32_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_i32(a i32, b i32) i32
    a + b

fn main()
    let r = add_i32(1000000, 1000000)
    let ok = if r == 2000000
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u8_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u8(x u8) u8
    x * 2

fn main()
    let r = double_u8(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u16_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u16(x u16) u16
    x * 2

fn main()
    let r = double_u16(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u32(x u32) u32
    x * 2

fn main()
    let r = double_u32(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u64(x u64) u64
    x * 2

fn main()
    let r = double_u64(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u8_roundtrip_within_signed_range() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_u8(x u8) u8
    x

fn main()
    let r = identity_u8(100)
    let ok = if r == 100
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}
