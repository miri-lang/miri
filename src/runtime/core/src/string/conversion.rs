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

/// Converts a boolean value to `"true"` or `"false"`.
///
/// Any non-zero `value` is treated as `true`.
#[no_mangle]
pub extern "C" fn miri_rt_bool_to_string(value: i64) -> *mut MiriString {
    let s = if value != 0 { "true" } else { "false" };
    into_raw_ptr(MiriString::from_str(s))
}
