// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_f32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_f32(x f32) f32
    x * 2.0

fn main()
    let r = double_f32(1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_f64(x f64) f64
    x * 2.0

fn main()
    let r = double_f64(1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f32_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_f32(a f32, b f32) f32
    a + b

fn main()
    let r = add_f32(1.5, 1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_f64(a f64, b f64) f64
    a + b

fn main()
    let r = add_f64(1.5, 1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_negative_param() {
    assert_runs_with_output(
        r#"
use system.io

fn negate_f64(x f64) f64
    -x

fn main()
    let r = negate_f64(2.5)
    let ok = if r == -2.5
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}
