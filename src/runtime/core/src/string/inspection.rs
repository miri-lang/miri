//! FFI functions for querying `MiriString` properties.
//!
//! All functions are null-safe: a null pointer is treated as an empty string.
//! Boolean results are returned as `u8` (0 = false, 1 = true) for C compatibility.

use super::{bool_to_ffi, deref_as_str, MiriString};

/// Returns the byte length of a string. Returns 0 for null pointers.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString`, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_len(ptr: *const MiriString) -> usize {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).len
}

/// Returns the Unicode scalar (character) count of a string.
///
/// This is an O(n) operation since UTF-8 characters are variable-width.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_char_count(ptr: *const MiriString) -> usize {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).as_str().chars().count()
}

/// Returns 1 if the string is empty (or null), 0 otherwise.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString`, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_is_empty(ptr: *const MiriString) -> u8 {
    if ptr.is_null() {
        return 1;
    }
    bool_to_ffi((*ptr).is_empty())
}

/// Returns 1 if `haystack` contains `needle` as a substring, 0 otherwise.
///
/// Returns 0 if either pointer is null.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_contains(
    haystack: *const MiriString,
    needle: *const MiriString,
) -> u8 {
    if haystack.is_null() || needle.is_null() {
        return 0;
    }
    bool_to_ffi((*haystack).as_str().contains((*needle).as_str()))
}

/// Returns 1 if the string starts with `prefix`, 0 otherwise.
///
/// Returns 0 if either pointer is null.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_starts_with(
    s: *const MiriString,
    prefix: *const MiriString,
) -> u8 {
    if s.is_null() || prefix.is_null() {
        return 0;
    }
    bool_to_ffi((*s).as_str().starts_with((*prefix).as_str()))
}

/// Returns 1 if the string ends with `suffix`, 0 otherwise.
///
/// Returns 0 if either pointer is null.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_ends_with(
    s: *const MiriString,
    suffix: *const MiriString,
) -> u8 {
    if s.is_null() || suffix.is_null() {
        return 0;
    }
    bool_to_ffi((*s).as_str().ends_with((*suffix).as_str()))
}

/// Returns 1 if both strings are byte-equal, 0 otherwise.
///
/// Two null pointers are considered equal (both represent empty strings).
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_equals(a: *const MiriString, b: *const MiriString) -> u8 {
    let a_str = deref_as_str(a);
    let b_str = deref_as_str(b);
    bool_to_ffi(a_str == b_str)
}

/// Returns the raw data pointer for a string, or null if `ptr` is null.
///
/// The returned pointer is valid only as long as the `MiriString` is alive.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString`, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_data(ptr: *const MiriString) -> *const u8 {
    if ptr.is_null() {
        return std::ptr::null();
    }
    (*ptr).data
}
