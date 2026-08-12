// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! String operations: split, join, and parsing.
//!
//! FFI functions that perform complex operations on strings.

use std::cell::RefCell;

use super::{into_raw_ptr, MiriString};
use crate::list::MiriList;

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
            let char_string = into_raw_ptr(MiriString::from_str(char_str));
            if !char_string.is_null() {
                crate::miri_rt_list_push(list, char_string as usize);
            }
        }
    } else {
        // Non-empty separator: split at occurrences
        for part in s_str.split(sep_str) {
            let part_string = into_raw_ptr(MiriString::from_str(part));
            if !part_string.is_null() {
                crate::miri_rt_list_push(list, part_string as usize);
            }
        }
    }

    // Set up drop function so removed elements are DecRef'd
    crate::miri_rt_list_set_elem_drop_fn(
        list,
        crate::miri_rt_list_decref_element as *const () as usize,
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
    let sep_str = if separator.is_null() {
        ""
    } else {
        (*separator).as_str()
    };

    if parts.is_null() {
        return into_raw_ptr(MiriString::from_str(""));
    }

    let parts_list = &*parts;
    if parts_list.is_empty() {
        return into_raw_ptr(MiriString::from_str(""));
    }

    // Read all strings from the list and collect into a Vec
    let mut parts_vec = Vec::new();
    for i in 0..parts_list.len() {
        // miri_rt_list_get returns a pointer to the storage where the element is stored.
        // Since the element size is sizeof(usize), we need to read a usize from this location.
        let elem_storage = crate::miri_rt_list_get(parts, i) as *const usize;
        if elem_storage.is_null() {
            continue;
        }
        let elem_ptr = *elem_storage as *const MiriString;
        if !elem_ptr.is_null() {
            let s = (*elem_ptr).as_str();
            parts_vec.push(s.to_string());
        }
    }

    if parts_vec.is_empty() {
        return into_raw_ptr(MiriString::from_str(""));
    }

    let joined = parts_vec.join(sep_str);
    into_raw_ptr(MiriString::from_str(&joined))
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
