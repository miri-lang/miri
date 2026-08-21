// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FFI functions that produce new `MiriString` values from existing ones.
//!
//! Every function in this module allocates and returns a fresh `MiriString`.
//! The caller is responsible for freeing the returned pointer via
//! [`super::miri_rt_string_free`].

use super::{into_raw_ptr, miri_rt_string_new, MiriString};
use crate::guard;

/// Concatenates two strings and returns a new string.
///
/// Handles null pointers gracefully — a null operand is treated as empty.
/// Returns an empty string on integer overflow or allocation failure.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_concat(
    left: *const MiriString,
    right: *const MiriString,
) -> *mut MiriString {
    guard::guard_check(left as *mut u8);
    guard::guard_check(right as *mut u8);
    let left_len = if left.is_null() { 0 } else { (*left).len };
    let right_len = if right.is_null() { 0 } else { (*right).len };

    if left_len == 0 && right_len == 0 {
        return miri_rt_string_new();
    }

    let total_len = match left_len.checked_add(right_len) {
        Some(total) => total,
        None => return miri_rt_string_new(),
    };

    // SAFETY: `total_len > 0` (at least one side is non-empty) and alignment 1 is valid.
    let data = crate::alloc::miri_alloc(total_len, 1);
    if data.is_null() {
        return miri_rt_string_new();
    }

    if left_len > 0 {
        // SAFETY: `left` is non-null (implied by `left_len > 0`), its `data` points to
        // `left_len` bytes. `data` has `total_len >= left_len` bytes. No overlap.
        std::ptr::copy_nonoverlapping((*left).data, data, left_len);
    }
    if right_len > 0 {
        // SAFETY: Same reasoning; destination starts at `data + left_len`.
        std::ptr::copy_nonoverlapping((*right).data, data.add(left_len), right_len);
    }

    into_raw_ptr(MiriString {
        data,
        len: total_len,
        capacity: total_len,
    })
}

/// Converts a string to lowercase (Unicode-aware).
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_to_lower(ptr: *const MiriString) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    transform_str(ptr, |s| s.to_lowercase())
}

/// Converts a string to uppercase (Unicode-aware).
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_to_upper(ptr: *const MiriString) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    transform_str(ptr, |s| s.to_uppercase())
}

/// Trims whitespace from both ends of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_trim(ptr: *const MiriString) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    transform_str_ref(ptr, str::trim)
}

/// Trims whitespace from the start (left side) of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_trim_start(ptr: *const MiriString) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    transform_str_ref(ptr, str::trim_start)
}

/// Trims whitespace from the end (right side) of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_trim_end(ptr: *const MiriString) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    transform_str_ref(ptr, str::trim_end)
}

/// Replaces all occurrences of `from` with `to` in the string.
///
/// If `from` is empty or null, returns a copy of the original string.
///
/// # Safety
/// - All pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_replace(
    s: *const MiriString,
    from: *const MiriString,
    to: *const MiriString,
) -> *mut MiriString {
    guard::guard_check(s as *mut u8);
    guard::guard_check(from as *mut u8);
    guard::guard_check(to as *mut u8);
    if s.is_null() {
        return miri_rt_string_new();
    }
    let str_val = (*s).as_str();
    let from_val = if from.is_null() { "" } else { (*from).as_str() };
    let to_val = if to.is_null() { "" } else { (*to).as_str() };

    if from_val.is_empty() {
        return into_raw_ptr(MiriString::from_str(str_val));
    }

    let replaced = str_val.replace(from_val, to_val);
    into_raw_ptr(MiriString::from_str(&replaced))
}

/// Returns a substring given byte indices `[start, end)`.
///
/// Returns an empty string if:
/// - `s` is null
/// - `start > end`
/// - `end` exceeds the string length
/// - `start` or `end` falls on a non-UTF-8-char boundary
///
/// # Safety
/// - `s` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_substring(
    s: *const MiriString,
    start: usize,
    end: usize,
) -> *mut MiriString {
    guard::guard_check(s as *mut u8);
    if s.is_null() {
        return miri_rt_string_new();
    }
    let str_val = (*s).as_str();

    if start > end || end > str_val.len() {
        return miri_rt_string_new();
    }
    if !str_val.is_char_boundary(start) || !str_val.is_char_boundary(end) {
        return miri_rt_string_new();
    }

    into_raw_ptr(MiriString::from_str(&str_val[start..end]))
}

/// Returns the character at the given Unicode scalar index as a single-character string.
///
/// This is O(n) because UTF-8 characters are variable-width.
/// Returns an empty string if the index is out of bounds or `ptr` is null.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_char_at(
    ptr: *const MiriString,
    index: usize,
) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    match s.chars().nth(index) {
        Some(ch) => {
            let mut buf = [0u8; 4];
            let char_str = ch.encode_utf8(&mut buf);
            into_raw_ptr(MiriString::from_str(char_str))
        }
        None => miri_rt_string_new(),
    }
}

/// Repeats a string `count` times.
///
/// Returns an empty string if `ptr` is null, `count` is 0, or the total length would overflow.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_repeat(
    ptr: *const MiriString,
    count: usize,
) -> *mut MiriString {
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() || count == 0 {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    // Guard against integer overflow: check that s.len() * count doesn't overflow.
    if s.len().checked_mul(count).is_none() {
        return miri_rt_string_new();
    }
    let repeated = s.repeat(count);
    into_raw_ptr(MiriString::from_str(&repeated))
}

/// Applies a transformation that produces an owned `String` from a `&str`.
///
/// Used by `to_lower`, `to_upper`, and similar functions.
///
/// # Safety
/// `ptr` must be a valid `MiriString` pointer or null.
unsafe fn transform_str(ptr: *const MiriString, transform: fn(&str) -> String) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let result = transform((*ptr).as_str());
    into_raw_ptr(MiriString::from_str(&result))
}

/// Applies a transformation that returns a `&str` slice of the original.
///
/// Used by `trim`, `trim_start`, `trim_end`.
///
/// # Safety
/// `ptr` must be a valid `MiriString` pointer or null.
unsafe fn transform_str_ref(
    ptr: *const MiriString,
    transform: fn(&str) -> &str,
) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let result = transform((*ptr).as_str());
    into_raw_ptr(MiriString::from_str(result))
}

/// Creates a string from a Unicode scalar value (code point).
///
/// Returns a new string containing the single character represented by the code point.
/// Returns an empty string if the code point is invalid (negative, surrogate, or > 0x10FFFF).
///
/// Takes `isize` rather than a fixed width because Miri's `int` lowers to the
/// platform pointer type.
#[no_mangle]
pub extern "C" fn miri_rt_string_from_code_point(code: isize) -> *mut MiriString {
    // Check for invalid code points: must be in range [0, 0x10FFFF] and not a surrogate
    if !(0..=0x10FFFF).contains(&code) || (0xD800..=0xDFFF).contains(&code) {
        return miri_rt_string_new();
    }

    if let Some(ch) = char::from_u32(code as u32) {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        into_raw_ptr(MiriString::from_str(encoded))
    } else {
        miri_rt_string_new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::constructors::miri_rt_string_free;
    use super::*;
    use crate::rc::balance_guard;

    #[test]
    fn repeat_with_huge_count_returns_empty() {
        let _balance = balance_guard();
        unsafe {
            // usize::MAX with any non-zero string length would overflow.
            let s = MiriString::from_str("hello");
            let ptr = into_raw_ptr(s);
            let result = miri_rt_string_repeat(ptr, usize::MAX);
            // Result should be an empty string.
            assert!((*result).len == 0, "huge count should return empty string");
            miri_rt_string_free(result);
            miri_rt_string_free(ptr);
        }
    }

    #[test]
    fn repeat_with_normal_count_works() {
        let _balance = balance_guard();
        unsafe {
            let s = MiriString::from_str("ab");
            let ptr = into_raw_ptr(s);
            let result = miri_rt_string_repeat(ptr, 3);
            assert_eq!((*result).len, 6, "3x 'ab' should be 6 bytes");
            let repeated_str = (*result).as_str();
            assert_eq!(repeated_str, "ababab", "3x 'ab' should be 'ababab'");
            miri_rt_string_free(result);
            miri_rt_string_free(ptr);
        }
    }

    #[test]
    fn repeat_with_zero_returns_empty() {
        let _balance = balance_guard();
        unsafe {
            let s = MiriString::from_str("hello");
            let ptr = into_raw_ptr(s);
            let result = miri_rt_string_repeat(ptr, 0);
            assert_eq!((*result).len, 0, "count 0 should return empty string");
            miri_rt_string_free(result);
            miri_rt_string_free(ptr);
        }
    }

    #[test]
    fn repeat_with_null_returns_empty() {
        let _balance = balance_guard();
        unsafe {
            let result = miri_rt_string_repeat(std::ptr::null(), 5);
            assert_eq!((*result).len, 0, "null input should return empty string");
            miri_rt_string_free(result);
        }
    }
}
