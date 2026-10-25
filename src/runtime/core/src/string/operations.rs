// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! String operations: split, join, and parsing.
//!
//! FFI functions that perform complex operations on strings.

use std::cell::RefCell;

use super::{alloc_string, new_string, new_string_of_ascii, MiriString};
use crate::guard;
use crate::list::{push_word, MiriList};

thread_local! {
    /// Thread-local status code for parsing operations.
    /// 0 = success, 1 = parse error
    static PARSE_STATUS: RefCell<i64> = const { RefCell::new(0) };
}

/// Sets the thread-local parse status code.
fn set_parse_status(code: i64) {
    PARSE_STATUS.with(|s| {
        *s.borrow_mut() = code;
    });
}

/// Returns the status code of the last parsing operation (int or float).
/// 0 = success, 1 = parse error.
///
/// This replaces both `miri_rt_string_parse_int_status` and
/// `miri_rt_string_parse_float_status`, which were identical.
#[no_mangle]
pub extern "C" fn miri_rt_string_parse_status() -> i64 {
    PARSE_STATUS.with(|s| *s.borrow())
}

/// Deprecated: use `miri_rt_string_parse_status` instead.
#[no_mangle]
pub extern "C" fn miri_rt_string_parse_int_status() -> i64 {
    miri_rt_string_parse_status()
}

/// Deprecated: use `miri_rt_string_parse_status` instead.
#[no_mangle]
pub extern "C" fn miri_rt_string_parse_float_status() -> i64 {
    miri_rt_string_parse_status()
}

/// Splits a string at each occurrence of the separator substring.
///
/// Empty separators split the string into individual characters.
/// Separators are removed from the result.
/// Empty fields are preserved.
///
/// # Safety
/// - Both `s` and `separator` must be valid `MiriString` pointers or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_split(
    s: *const MiriString,
    separator: *const MiriString,
) -> *mut MiriList {
    guard::guard_check(s as *mut u8);
    guard::guard_check(separator as *mut u8);
    let s_str = if s.is_null() { "" } else { (*s).as_str() };
    let sep_str = if separator.is_null() {
        ""
    } else {
        (*separator).as_str()
    };

    let list = crate::miri_rt_list_new(std::mem::size_of::<*mut u8>());
    if list.is_null() {
        return list;
    }

    // Empty separator: split into characters
    if sep_str.is_empty() {
        for ch in s_str.chars() {
            let mut buf = [0u8; 4];
            let char_str = ch.encode_utf8(&mut buf);
            let char_string = new_string(char_str);
            if !char_string.is_null() {
                push_word(list, char_string as usize);
            }
        }
    } else {
        // Every piece of an ASCII string is ASCII, so none is scanned again.
        let ascii = s.is_null() || (*s).ascii;
        let push_piece = |part: &str| {
            let part_string = new_string_of_ascii(part, ascii || part.is_ascii());
            if !part_string.is_null() {
                push_word(list, part_string as usize);
            }
        };
        match sep_str.as_bytes() {
            // A one-byte separator is ASCII, so a byte equal to it is a whole
            // character and every piece between two of them is UTF-8: scan the
            // bytes rather than search for a pattern.
            [byte] => {
                let pieces = s_str.bytes().filter(|b| b == byte).count() + 1;
                crate::list::ffi::miri_rt_list_reserve(list, pieces as i64);
                for part in s_str.as_bytes().split(|b| b == byte) {
                    push_piece(std::str::from_utf8_unchecked(part));
                }
            }
            _ => s_str.split(sep_str).for_each(push_piece),
        }
    }

    // Every element is a string: one removed from the list is released as one.
    crate::miri_rt_list_set_elem_drop_fn(
        list,
        super::constructors::miri_rt_string_decref_element as *const () as usize,
    );

    list
}

/// Joins a list of strings with this string as the separator.
///
/// The receiver is the separator (Python-style).
/// An empty list returns an empty string.
/// A single-element list returns that element unchanged.
///
/// # Safety
/// - `separator` must be a valid `MiriString` pointer or null.
/// - `parts` must be a valid `MiriList` pointer or null.
///   The function does NOT take ownership of the list or its elements.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_join(
    separator: *const MiriString,
    parts: *const MiriList,
) -> *mut MiriString {
    guard::guard_check(separator as *mut u8);
    let sep_str = if separator.is_null() {
        ""
    } else {
        (*separator).as_str()
    };

    if parts.is_null() {
        return new_string("");
    }

    let parts: Vec<&str> = (0..(*parts).len())
        .filter_map(|i| list_string_at(parts, i))
        .collect();
    join_into_new_string(sep_str, &parts)
}

/// The string the list element at `index` points at, or `None` when the slot
/// or the element is null.
///
/// # Safety
/// `parts` is a live list of string pointers and `index` is in bounds.
unsafe fn list_string_at<'a>(parts: *const MiriList, index: usize) -> Option<&'a str> {
    // The slot holds one word: the element's pointer.
    let slot = crate::miri_rt_list_get(parts, index) as *const usize;
    if slot.is_null() {
        return None;
    }
    let element = *slot as *const MiriString;
    (!element.is_null()).then(|| (*element).as_str())
}

/// `parts` joined by `separator` in a new string, written straight into the
/// string's buffer, so the result is held once and never built in a Rust
/// `String` first.
fn join_into_new_string(separator: &str, parts: &[&str]) -> *mut MiriString {
    let separators = separator.len().checked_mul(parts.len().saturating_sub(1));
    let Some(len) = parts
        .iter()
        .try_fold(separators.unwrap_or(usize::MAX), |sum, part| {
            sum.checked_add(part.len())
        })
        .filter(|&len| len < isize::MAX as usize)
    else {
        return std::ptr::null_mut();
    };
    let ascii =
        (parts.len() < 2 || separator.is_ascii()) && parts.iter().all(|part| part.is_ascii());
    // SAFETY: the string has room for `len` bytes, and the loop writes exactly
    // `len` bytes of valid UTF-8 (whole parts and separators), ASCII exactly
    // when `ascii` says so; no part lies inside the block just allocated.
    unsafe {
        let string = alloc_string(len, ascii);
        if string.is_null() || len == 0 {
            return string;
        }
        let mut at = (*string).data;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                std::ptr::copy_nonoverlapping(separator.as_ptr(), at, separator.len());
                at = at.add(separator.len());
            }
            std::ptr::copy_nonoverlapping(part.as_ptr(), at, part.len());
            at = at.add(part.len());
        }
        string
    }
}

/// Parses a string as an integer.
///
/// Returns the parsed integer on success (status = 0).
/// Returns 0 on parse failure (status = 1).
///
/// Whitespace is NOT trimmed.
///
/// # Safety
/// - `s` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_parse_int(s: *const MiriString) -> i64 {
    guard::guard_check(s as *mut u8);
    let s_str = if s.is_null() { "" } else { (*s).as_str() };

    match s_str.parse::<i64>() {
        Ok(n) => {
            set_parse_status(0);
            n
        }
        Err(_) => {
            set_parse_status(1);
            0
        }
    }
}

/// Parses a string as a floating-point number.
///
/// Returns the parsed float on success (status = 0).
/// Returns 0.0 on parse failure (status = 1).
///
/// Whitespace is NOT trimmed.
///
/// # Safety
/// - `s` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_parse_float(s: *const MiriString) -> f64 {
    guard::guard_check(s as *mut u8);
    let s_str = if s.is_null() { "" } else { (*s).as_str() };

    match s_str.parse::<f64>() {
        Ok(f) => {
            set_parse_status(0);
            f
        }
        Err(_) => {
            set_parse_status(1);
            0.0
        }
    }
}

/// Returns the byte offset of the next UTF-8 character boundary.
///
/// Given a byte offset, returns the byte offset of the start of the next UTF-8 character.
/// If the offset is already at or past the end of the string, returns the string length.
///
/// # Safety
/// - `s` must be a valid `MiriString` pointer or null.
/// - `byte_pos` must be >= 0.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_next_char_boundary(
    s: *const MiriString,
    byte_pos: i64,
) -> i64 {
    guard::guard_check(s as *mut u8);
    let s_str = if s.is_null() { "" } else { (*s).as_str() };
    let len = s_str.len() as i64;

    if byte_pos < 0 || byte_pos >= len {
        return len;
    }

    let pos = byte_pos as usize;
    let bytes = s_str.as_bytes();

    let mut next_pos = pos + 1;
    while next_pos < bytes.len() && (bytes[next_pos] & 0xC0) == 0x80 {
        next_pos += 1;
    }

    next_pos as i64
}
