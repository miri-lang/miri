// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Casts between the 128-bit integers and the floats.
//!
//! A float converts to an integer by truncating toward zero and saturating at
//! the integer's bounds, with NaN converting to zero — the rule every narrower
//! width follows. An integer converts to the nearest float. Values past 2^64
//! are used wherever the high word matters, so a conversion that drops it
//! shows up in the output.

use super::super::utils::*;

#[test]
fn test_cast_f64_to_i128_truncates_toward_zero() {
    assert_runs_with_output(
        r#"
        let x f64 = 3.75
        let neg f64 = -3.75
        let y = x as i128
        let z = neg as i128
        println(f'{y} {z}')
        "#,
        "3 -3",
    );
}

#[test]
fn test_cast_f64_past_the_low_word_to_i128_keeps_the_high_word() {
    assert_runs_with_output(
        r#"
        let x f64 = 1e20
        let y = x as i128
        println(f'{y}')
        "#,
        "100000000000000000000",
    );
}

#[test]
fn test_cast_nan_to_i128_is_zero() {
    assert_runs_with_output(
        r#"
        let zero f64 = 0.0
        let nan = zero / zero
        let y = nan as i128
        let z = nan as u128
        println(f'{y} {z}')
        "#,
        "0 0",
    );
}

#[test]
fn test_cast_f64_to_i128_saturates_at_the_bounds() {
    assert_runs_with_output(
        r#"
        let big f64 = 1e40
        let small f64 = -1e40
        let hi = big as i128
        let lo = small as i128
        println(f'{hi} {lo}')
        "#,
        "170141183460469231731687303715884105727 -170141183460469231731687303715884105728",
    );
}

#[test]
fn test_cast_f64_to_u128_saturates_at_the_bounds() {
    assert_runs_with_output(
        r#"
        let big f64 = 1e40
        let neg f64 = -3.75
        let hi = big as u128
        let lo = neg as u128
        println(f'{hi} {lo}')
        "#,
        "340282366920938463463374607431768211455 0",
    );
}

#[test]
fn test_cast_f32_to_i128_and_u128() {
    assert_runs_with_output(
        r#"
        let x f32 = 2.5
        let y = x as i128
        let z = x as u128
        println(f'{y} {z}')
        "#,
        "2 2",
    );
}

#[test]
fn test_cast_i128_past_the_low_word_to_f64() {
    assert_runs_with_output(
        r#"
        let a i128 = 36893488147419103232
        let n i128 = -36893488147419103232
        let b = a as f64
        let c = n as float
        println(f'{b} {c}')
        "#,
        "36893488147419103232.0 -36893488147419103232.0",
    );
}

#[test]
fn test_cast_i128_to_float_small_value() {
    assert_runs_with_output(
        r#"
        let a i128 = 42
        let b = a as float
        println(f'{b}')
        "#,
        "42.0",
    );
}

#[test]
fn test_cast_u128_past_the_low_word_to_f64_and_f32() {
    assert_runs_with_output(
        r#"
        let a u128 = 36893488147419103232
        let b = a as f64
        let c = a as f32
        println(f'{b} {c}')
        "#,
        "36893488147419103232.0 36893488147419103232.0",
    );
}

#[test]
fn test_cast_i128_to_f32() {
    assert_runs_with_output(
        r#"
        let a i128 = -42
        let b = a as f32
        println(f'{b}')
        "#,
        "-42.0",
    );
}

#[test]
fn test_cast_bool_to_i128_is_rejected() {
    assert_compiler_error(
        r#"
        let x = true as i128
        "#,
        "cast",
    );
}
