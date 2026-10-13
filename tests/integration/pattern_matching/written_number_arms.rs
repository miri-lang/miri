// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An arm whose value is a number written in the source takes the type the
//! other arms settle, as a literal takes the type of the other operand.

use super::utils::*;

#[test]
fn a_literal_arm_after_an_f32_arm_is_an_f32() {
    assert_runs_with_output(
        r#"
fn half(x f32?) f32
    match x
        Some(v): v / 2.0
        None: 0.0

fn main()
    let a f32 = 3.0
    println(f"{half(Some(a))} {half(None)}")
"#,
        "1.5 0.0",
    );
}

#[test]
fn a_literal_arm_before_an_f32_arm_is_an_f32() {
    assert_runs_with_output(
        r#"
fn half(x f32?) f32
    match x
        None: -1.0
        Some(v): v / 2.0

fn main()
    let a f32 = 3.0
    println(f"{half(Some(a))} {half(None)}")
"#,
        "1.5 -1.0",
    );
}

#[test]
fn literal_arithmetic_in_an_arm_takes_the_narrow_integer_width() {
    assert_runs_with_output(
        r#"
fn code(x i8?) i8
    match x
        Some(v): v
        None: 2 * 3

fn main()
    let a i8 = 4
    println(f"{code(Some(a))} {code(None)}")
"#,
        "4 6",
    );
}

#[test]
fn a_float_value_arm_beside_an_f32_arm_is_still_refused() {
    assert_compiler_error(
        r#"
fn pick(x f32?, w float) f32
    match x
        Some(v): v
        None: w

fn main()
    let a f32 = 3.0
    println(f"{pick(Some(a), 1.0)}")
"#,
        "Match branch types mismatch: expected f32, got float",
    );
}
