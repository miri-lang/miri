// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FFI functions for converting primitive types to `MiriString`.
//!
//! These functions are called from compiled Miri code during formatted string
//! interpolation (e.g., `"value is {x}"` where `x` is an `Int`, `Float`, or `Bool`).

use super::{into_raw_ptr, MiriString};

/// Converts a 64-bit signed integer to its decimal string representation.
#[no_mangle]
pub extern "C" fn miri_rt_int_to_string(value: i64) -> *mut MiriString {
    let s = value.to_string();
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a 64-bit unsigned integer to its decimal string representation.
///
/// The compiler routes unsigned integer types here so that a value with the
/// high bit set (>= 2^63) formats as its unsigned magnitude rather than being
/// reinterpreted as a negative `i64`. The bits are passed through unchanged from
/// the codegen side, which is why the parameter is taken as `u64`.
#[no_mangle]
pub extern "C" fn miri_rt_uint_to_string(value: u64) -> *mut MiriString {
    let s = value.to_string();
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a 64-bit float to its string representation.
///
/// Whole-number floats are formatted with one decimal place (e.g., `3.0` instead
/// of `3`) to distinguish them from integers. Non-finite values (`NaN`, `inf`)
/// use Rust's default formatting.
#[no_mangle]
pub extern "C" fn miri_rt_float_to_string(value: f64) -> *mut MiriString {
    let s = if value.fract() == 0.0 && value.is_finite() {
        format!("{value:.1}")
    } else {
        value.to_string()
    };
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a 32-bit float to its string representation.
///
/// Formatted from the `f32` directly rather than a promoted `f64` so the
/// shortest round-trip representation of the `f32` is used (`0.1f32` renders as
/// `0.1`, not the `0.10000000149011612` that promoting to `f64` would expose).
/// Whole-number values keep the one-decimal-place convention (e.g. `3.0`).
#[no_mangle]
pub extern "C" fn miri_rt_f32_to_string(value: f32) -> *mut MiriString {
    let s = if value.fract() == 0.0 && value.is_finite() {
        format!("{value:.1}")
    } else {
        value.to_string()
    };
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a boolean value to `"true"` or `"false"`.
///
/// Any non-zero `value` is treated as `true`.
#[no_mangle]
pub extern "C" fn miri_rt_bool_to_string(value: i64) -> *mut MiriString {
    let s = if value != 0 { "true" } else { "false" };
    into_raw_ptr(MiriString::from_str(s))
}
