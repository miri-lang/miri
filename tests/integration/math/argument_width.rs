// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every `system.math` function takes a `Real` and returns the width of its
//! argument: `sin` of an `f32` is an `f32`, of an `f64` an `f64`, of a `float`
//! a `float`, so code at one width needs no conversion around a math call.

use super::super::utils::*;

#[test]
fn a_math_function_of_an_f32_is_an_f32() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let x f32 = 16.0
    let root f32 = sqrt(x)
    let zero f32 = sin(x - x)
    let edge f32 = clamp(x, 0.0, 1.0)
    let half f32 = mix(x, 0.0, 0.5)
    println(f"{root} {zero} {edge} {half}")
"#,
        "4.0 0.0 1.0 8.0",
    );
}

#[test]
fn a_math_function_of_an_f64_is_an_f64() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let x f64 = 2.0
    let power f64 = pow(x, 10.0)
    let one f64 = cos(x - x)
    println(f"{power} {one}")
"#,
        "1024.0 1.0",
    );
}

#[test]
fn a_math_function_of_a_float_is_a_float() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let y float = floor(2.5) + exp(0.0)
    println(f"{y}")
"#,
        "3.0",
    );
}

/// The set a selectively imported function is bounded by is reached through
/// the function, so naming the function alone is enough.
#[test]
fn a_selectively_imported_math_function_keeps_its_argument_width() {
    assert_runs_with_output(
        r#"
use system.io
use system.math.{sqrt, sigmoid}

fn main()
    let x f32 = 9.0
    let root f32 = sqrt(x)
    let mid f32 = sigmoid(x - x)
    println(f"{root} {mid}")
"#,
        "3.0 0.5",
    );
}

/// A math function never narrows: its result is as wide as its argument.
#[test]
fn a_math_result_wider_than_its_target_is_refused() {
    assert_compiler_error(
        r#"
use system.math

fn main()
    let x f64 = 2.0
    let y f32 = sqrt(x)
"#,
        "f64",
    );
}

/// The arguments of one call are one width; mixing two is refused rather than
/// silently converting either.
#[test]
fn math_arguments_at_two_widths_are_refused() {
    assert_compiler_error(
        r#"
use system.math

fn main()
    let a f32 = 1.0
    let b f64 = 2.0
    let c = pow(a, b)
"#,
        "pow",
    );
}

/// A type outside the set is refused at the call.
#[test]
fn a_math_function_refuses_a_string() {
    assert_compiler_error(
        r#"
use system.math

fn main()
    let s = sin("x")
"#,
        "Real",
    );
}

/// Literal arguments take the width of the value beside them, wherever it is
/// written.
#[test]
fn literal_math_arguments_take_the_width_of_the_value_beside_them() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let t f32 = 0.25
    let a f32 = mix(0.0, 4.0, t)
    let b f32 = smoothstep(0.0, 1.0, t)
    println(f"{a} {b}")
"#,
        "1.0 0.15625",
    );
}
