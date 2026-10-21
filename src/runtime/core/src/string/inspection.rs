// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FFI functions for querying `MiriString` properties.
//!
//! All functions are null-safe: a null pointer is treated as an empty string.
//! Boolean results are returned as `u8` (0 = false, 1 = true) for C compatibility.

use super::{bool_to_ffi, deref_as_str, MiriString};
use crate::guard;

/// Returns the byte length of a string. Returns 0 for null pointers.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString`, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_len(ptr: *const MiriString) -> usize {
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() {
        return 0;
    }
    (*ptr).len
}

/// Returns the Unicode scalar (character) count of a string.
///
/// Counting is O(n) since UTF-8 characters are variable-width, so the count
/// is remembered: asking again for the same string is O(1).
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_char_count(ptr: *const MiriString) -> usize {
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() {
        return 0;
    }
    super::char_index::char_count(&*ptr)
}

/// Returns 1 if the string is empty (or null), 0 otherwise.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString`, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_is_empty(ptr: *const MiriString) -> u8 {
    guard::guard_check(ptr as *mut u8);
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
    guard::guard_check(haystack as *mut u8);
    guard::guard_check(needle as *mut u8);
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
    guard::guard_check(s as *mut u8);
    guard::guard_check(prefix as *mut u8);
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
    guard::guard_check(s as *mut u8);
    guard::guard_check(suffix as *mut u8);
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
    guard::guard_check(a as *mut u8);
    guard::guard_check(b as *mut u8);
    let a_str = deref_as_str(a);
    let b_str = deref_as_str(b);
    bool_to_ffi(a_str == b_str)
}

/// Orders two strings by content, returning -1, 0 or 1.
///
/// The comparison is over the UTF-8 bytes, which for Unicode text orders by
/// code point. A null pointer is treated as the empty string, so two nulls
/// compare equal and a null sorts before any non-empty string.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_compare(
    a: *const MiriString,
    b: *const MiriString,
) -> isize {
    guard::guard_check(a as *mut u8);
    guard::guard_check(b as *mut u8);
    match deref_as_str(a).cmp(deref_as_str(b)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
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
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() {
        return std::ptr::null();
    }
    (*ptr).data
}

/// Returns the Unicode scalar value (code point) at a CHARACTER index.
///
/// Returns the code point (0-0x10FFFF) on success, or -1 if the string is null,
/// the index is negative, or the index is out of range. The caller should check
/// for -1 to determine if the result is valid.
///
/// Takes and returns `isize` rather than a fixed width because Miri's `int`
/// lowers to the platform pointer type.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_code_at(ptr: *const MiriString, index: isize) -> isize {
    guard::guard_check(ptr as *mut u8);
    if ptr.is_null() || index < 0 {
        return -1;
    }
    match super::char_index::char_at(&*ptr, index as usize) {
        Some(ch) => ch as isize,
        None => -1,
    }
}

/// Returns the CHARACTER index of the first character of `ptr`, at or after
/// character `start`, that is one of the characters of `chars`.
///
/// Returns -1 when there is none: `chars` is empty, `start` is negative or at
/// or past the end, or either pointer is null.
///
/// A set of ASCII characters is matched byte by byte, which is sound in any
/// text: every byte of a multi-byte UTF-8 character is 0x80 or above, so it
/// never equals an ASCII byte. The start is turned into a byte offset, and the
/// match back into a character index, by the remembered character positions,
/// so a run of searches moving forward through one text steps over each
/// character once in total.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_string_index_of_any(
    ptr: *const MiriString,
    chars: *const MiriString,
    start: isize,
) -> isize {
    guard::guard_check(ptr as *mut u8);
    guard::guard_check(chars as *mut u8);
    if ptr.is_null() || chars.is_null() || start < 0 {
        return -1;
    }
    let (text, set) = (&*ptr, &*chars);
    let Some(from) = super::char_index::char_byte_offset(text, start as usize) else {
        return -1;
    };
    let rest = text.as_str().get(from..).unwrap_or("");
    let found = if set.ascii {
        position_of_ascii_set(rest.as_bytes(), set.as_str().as_bytes())
    } else {
        position_of_char_set(rest, set.as_str())
    };
    match found {
        Some(offset) => super::char_index::char_index_of_byte(text, from + offset) as isize,
        None => -1,
    }
}

/// The byte offset of the first byte of `text` that is one of the ASCII bytes
/// of `set`.
fn position_of_ascii_set(text: &[u8], set: &[u8]) -> Option<usize> {
    if let [only] = set {
        return text.iter().position(|byte| byte == only);
    }
    let members = set
        .iter()
        .fold(0u128, |members, &byte| members | (1u128 << (byte & 0x7f)));
    text.iter()
        .position(|&byte| byte < 0x80 && (members >> byte) & 1 == 1)
}

/// The byte offset of the first character of `text` that is a character of
/// `set`.
fn position_of_char_set(text: &str, set: &str) -> Option<usize> {
    text.char_indices()
        .find(|&(_, ch)| set.contains(ch))
        .map(|(offset, _)| offset)
}
