// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FFI functions for converting primitive types to `MiriString`.
//!
//! These functions are called from compiled Miri code during formatted string
//! interpolation (e.g., `"value is {x}"` where `x` is an `Int`, `Float`, or `Bool`).

use super::{into_raw_ptr, MiriString};

/// Converts a 64-bit signed integer to its decimal string representation.
///
/// Integer widths up to 64 bits are widened to this parameter at the call. The
/// 128-bit widths have conversions of their own — they cannot travel in a value
/// word, and widening this one would change the ABI every narrower width
/// already calls with.
#[no_mangle]
pub extern "C" fn miri_rt_int_to_string(value: i64) -> *mut MiriString {
    let s = value.to_string();
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a 128-bit signed integer to its decimal string representation.
///
/// The value arrives by address because 128 bits do not fit the value word the
/// narrower conversions are called with. Reading it back through the pointer
/// keeps the bytes the caller wrote, whatever register pair the target would
/// otherwise have split them across.
///
/// # Safety
///
/// `value_ptr` must point to 16 readable bytes holding the value. The compiler
/// only ever emits this call against the address of a stack slot it just wrote.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_i128_to_string(value_ptr: *const i128) -> *mut MiriString {
    let s = value_ptr.read_unaligned().to_string();
    into_raw_ptr(MiriString::from_str(&s))
}

/// Converts a 128-bit unsigned integer to its decimal string representation.
///
/// Taken by address for the same reason as the signed conversion, and read as
/// `u128` so a value with the high bit set formats as its magnitude rather than
/// a negative `i128`.
///
/// # Safety
///
/// `value_ptr` must point to 16 readable bytes holding the value. The compiler
/// only ever emits this call against the address of a stack slot it just wrote.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_u128_to_string(value_ptr: *const u128) -> *mut MiriString {
    let s = value_ptr.read_unaligned().to_string();
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
