// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Regex compilation and matching utilities.
//!
//! Provides FFI functions for regex pattern compilation and matching using
//! the `regex` crate. Patterns are compiled once and cached in a thread-local
//! slab to avoid recompilation overhead.

use std::cell::RefCell;

use crate::string::MiriString;

thread_local! {
    static REGEX_STATE: RefCell<RegexState> = const { RefCell::new(RegexState {
        slab: Vec::new(),
        compile_status: 0,
        compile_message: None,
        match_result: (0, 0),
    }) };
}

struct RegexState {
    slab: Vec<regex::Regex>,
    compile_status: i64,
    compile_message: Option<String>,
    match_result: (i64, i64),
}

/// Sets the compile status code and message.
fn set_compile_error(status: i64, msg: String) {
    REGEX_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.compile_status = status;
        s.compile_message = Some(msg);
    });
}

/// Marks compilation as successful.
fn set_compile_success() {
    REGEX_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.compile_status = 0;
        s.compile_message = None;
    });
}

/// Sets the match result (start and end byte offsets).
fn set_match_result(start: i64, end: i64) {
    REGEX_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.match_result = (start, end);
    });
}

/// Compiles a regex pattern and stores it in the slab.
///
/// Returns a handle (index into the slab) if successful.
/// On error, returns -1 and sets the compile status and message.
/// Status codes: 0 = success, 1 = syntax error, 2 = size exceeded, 3 = other error.
///
/// # Safety
/// - `pattern` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_compile(pattern: *const MiriString) -> i64 {
    let pattern_str = if pattern.is_null() {
        ""
    } else {
        (*pattern).as_str()
    };

    match regex::Regex::new(pattern_str) {
        Ok(regex) => {
            let handle = REGEX_STATE.with(|state| {
                let mut s = state.borrow_mut();
                let handle = s.slab.len() as i64;
                s.slab.push(regex);
                handle
            });
            set_compile_success();
            handle
        }
        Err(e) => {
            let msg = e.to_string();
            let status = match e {
                regex::Error::Syntax(_) => 1,
                regex::Error::CompiledTooBig(_) => 2,
                _ => 3,
            };
            set_compile_error(status, msg);
            -1
        }
    }
}

/// Returns the status code of the last compile operation.
///
/// Status codes:
/// - 0 = success
/// - 1 = syntax error (invalid pattern)
/// - 2 = size exceeded (compiled pattern too large)
/// - 3 = other error
#[no_mangle]
pub extern "C" fn miri_rt_regex_compile_status() -> i64 {
    REGEX_STATE.with(|state| state.borrow().compile_status)
}

/// Returns the error message of the last compile operation as a MiriString.
///
/// Allocates a new MiriString for each call. If the last compile succeeded,
/// returns a null pointer.
/// The caller does NOT take ownership of the returned string (Miri RC management applies).
#[no_mangle]
pub extern "C" fn miri_rt_regex_compile_message() -> *mut MiriString {
    REGEX_STATE.with(|state| match state.borrow().compile_message.as_ref() {
        Some(msg) => crate::string::into_raw_ptr(MiriString::from_str(msg)),
        None => std::ptr::null_mut(),
    })
}

/// Tests if the regex matches anywhere in the input string.
///
/// Returns true if the pattern matches any substring; false otherwise.
/// On invalid handle, returns false.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_matches(handle: i64, text: *const MiriString) -> bool {
    let text_str = if text.is_null() { "" } else { (*text).as_str() };

    REGEX_STATE.with(|state| {
        let s = state.borrow();
        if handle < 0 || handle >= s.slab.len() as i64 {
            return false;
        }
        s.slab[handle as usize].is_match(text_str)
    })
}

/// Finds the first match of the regex in the input string.
///
/// Returns true if a match is found; false otherwise.
/// If a match is found, the start and end byte offsets are stored in thread-locals
/// and can be retrieved via `miri_rt_regex_match_start()` and `miri_rt_regex_match_end()`.
/// On invalid handle, returns false.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_find(handle: i64, text: *const MiriString) -> bool {
    let text_str = if text.is_null() { "" } else { (*text).as_str() };

    let match_info = REGEX_STATE.with(|state| {
        let s = state.borrow();
        if handle < 0 || handle >= s.slab.len() as i64 {
            return None;
        }

        s.slab[handle as usize]
            .find(text_str)
            .map(|m| (m.start() as i64, m.end() as i64))
    });

    if let Some((start, end)) = match_info {
        set_match_result(start, end);
        true
    } else {
        false
    }
}

/// Finds the first match of the regex starting from a given byte offset.
///
/// Returns true if a match is found; false otherwise.
/// If a match is found, the start and end byte offsets are stored in thread-locals
/// and can be retrieved via `miri_rt_regex_match_start()` and `miri_rt_regex_match_end()`.
/// On invalid handle or if `from` >= text length, returns false.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
/// - `from` must be a valid byte offset at a UTF-8 boundary.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_find_from(
    handle: i64,
    text: *const MiriString,
    from: i64,
) -> bool {
    let text_str = if text.is_null() { "" } else { (*text).as_str() };

    if from < 0 || from as usize > text_str.len() {
        return false;
    }

    let start_idx = from as usize;

    // Check if start_idx is at a UTF-8 character boundary
    if start_idx > 0 && start_idx < text_str.len() {
        let bytes = text_str.as_bytes();
        if (bytes[start_idx] & 0xC0) == 0x80 {
            return false;
        }
    }

    let match_info = REGEX_STATE.with(|state| {
        let s = state.borrow();
        if handle < 0 || handle >= s.slab.len() as i64 {
            return None;
        }

        let text_slice = &text_str[start_idx..];
        s.slab[handle as usize]
            .find(text_slice)
            .map(|m| ((start_idx + m.start()) as i64, (start_idx + m.end()) as i64))
    });

    if let Some((start, end)) = match_info {
        set_match_result(start, end);
        true
    } else {
        false
    }
}

/// Returns the start byte offset of the last match found by `miri_rt_regex_find()`.
#[no_mangle]
pub extern "C" fn miri_rt_regex_match_start() -> i64 {
    REGEX_STATE.with(|state| state.borrow().match_result.0)
}

/// Returns the end byte offset of the last match found by `miri_rt_regex_find()`.
#[no_mangle]
pub extern "C" fn miri_rt_regex_match_end() -> i64 {
    REGEX_STATE.with(|state| state.borrow().match_result.1)
}

/// Replaces all non-overlapping matches of the regex in the input string.
///
/// Returns a new string with all matches replaced by `replacement`.
/// The replacement is treated as a literal string; no capture-group substitution is performed.
/// On invalid handle, returns the input string unchanged.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
/// - `replacement` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_replace(
    handle: i64,
    text: *const MiriString,
    replacement: *const MiriString,
) -> *mut MiriString {
    if handle < 0 {
        let text_str = if text.is_null() { "" } else { (*text).as_str() };
        return crate::string::into_raw_ptr(MiriString::from_str(text_str));
    }

    let text_str = if text.is_null() { "" } else { (*text).as_str() };
    let repl_str = if replacement.is_null() {
        ""
    } else {
        (*replacement).as_str()
    };

    REGEX_STATE.with(|state| {
        let s = state.borrow();
        if handle >= s.slab.len() as i64 {
            return crate::string::into_raw_ptr(MiriString::from_str(text_str));
        }
        let result = s.slab[handle as usize]
            .replace_all(text_str, regex::NoExpand(repl_str))
            .into_owned();
        crate::string::into_raw_ptr(MiriString::from_str(&result))
    })
}

pub mod ffi {
    pub use super::{
        miri_rt_regex_compile, miri_rt_regex_compile_message, miri_rt_regex_compile_status,
        miri_rt_regex_find, miri_rt_regex_find_from, miri_rt_regex_match_end,
        miri_rt_regex_match_start, miri_rt_regex_matches, miri_rt_regex_replace,
    };
}
