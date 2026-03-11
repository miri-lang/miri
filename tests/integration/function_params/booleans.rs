// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_bool_identity_param() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_bool(b bool) bool
    b

fn main()
    let r = if identity_bool(true)
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_bool_negate_param() {
    assert_runs_with_output(
        r#"
use system.io

fn logical_not(b bool) bool
    not b

fn main()
    let a = if logical_not(false)
        1
    else
        0
    let b = if logical_not(true)
        0
    else
        1
    println(f"{a + b}")
"#,
        "2",
    );
}

#[test]
fn test_bool_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn both_true(a bool, b bool) bool
    a and b

fn main()
    let r = if both_true(true, true)
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_bool_param_false() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_bool(b bool) bool
    b

fn main()
    let r = if identity_bool(false)
        1
    else
        0
    println(f"{r}")
"#,
        "0",
    );
}
