// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_int_and_bool_params() {
    assert_runs_with_output(
        r#"
use system.io

fn conditional_double(x int, flag bool) int
    if flag
        x * 2
    else
        x

fn main()
    let a = conditional_double(21, true)
    let b = conditional_double(21, false)
    println(f"{a}")
    println(f"{b}")
"#,
        "42\n21", // Original was "42"
    );
}

#[test]
fn test_multiple_same_width_int_params() {
    assert_runs_with_output(
        r#"
use system.io

fn sum_i32(a i32, b i32, c i32) i32
    a + b + c

fn main()
    let r = sum_i32(1, 10, 100)
    let ok = if r == 111
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}
