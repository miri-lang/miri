// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Regex compilation and matching utilities.
//!
//! Provides FFI functions for regex pattern compilation and matching using
//! the `regex` crate. Patterns are compiled once and cached in a thread-local
//! slab to avoid recompilation overhead.

use crate::guard;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::string::MiriString;

thread_local! {
    static REGEX_STATE: RefCell<RegexState> = RefCell::new(RegexState {
        slab: Vec::new(),
        pattern_cache: HashMap::new(),
        compile_status: 0,
        compile_message: None,
        match_result: (0, 0),
    });
}

struct RegexState {
    slab: Vec<regex::Regex>,
    /// Maps compiled pattern strings to their slab handles.
    /// Used to avoid recompiling the same pattern repeatedly.
    pattern_cache: HashMap<String, i64>,
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
/// If the pattern has already been compiled, returns the cached handle.
///
/// # Safety
/// - `pattern` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_compile(pattern: *const MiriString) -> i64 {
    guard::guard_check(pattern as *mut u8);
    let pattern_str = if pattern.is_null() {
        "".to_string()
    } else {
        (*pattern).as_str().to_string()
    };

    // Check cache first
    let cached = REGEX_STATE.with(|state| state.borrow().pattern_cache.get(&pattern_str).copied());
    if let Some(cached_handle) = cached {
        set_compile_success();
        return cached_handle;
    }

    match regex::Regex::new(&pattern_str) {
        Ok(regex) => {
            let handle = REGEX_STATE.with(|state| {
                let mut s = state.borrow_mut();
                let handle = s.slab.len() as i64;
                s.slab.push(regex);
                s.pattern_cache.insert(pattern_str, handle);
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
        Some(msg) => crate::string::new_string(msg),
        None => std::ptr::null_mut(),
    })
}

/// Compiles a regex pattern that the compiler has already validated.
///
/// This function is called by MIR lowering when constructing regex literals.
/// Because the compiler validated the pattern at compile time, a failure
/// here indicates a version mismatch between the compiler's and runtime's
/// regex crate versions. In that case, the runtime aborts securely with a trap.
///
/// Patterns are cached to avoid recompilation across multiple evaluations
/// (e.g. inside a loop).
///
/// # Safety
/// - `pattern` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_from_validated_pattern(pattern: *const MiriString) -> i64 {
    guard::guard_check(pattern as *mut u8);
    let pattern_str = if pattern.is_null() {
        ""
    } else {
        (*pattern).as_str()
    };

    // Check cache first without allocating a String unless a miss occurs.
    let cached = REGEX_STATE.with(|state| {
        state
            .borrow()
            .pattern_cache
            .iter()
            .find(|(k, _)| k.as_str() == pattern_str)
            .map(|(_, &handle)| handle)
    });
    if let Some(cached_handle) = cached {
        return cached_handle;
    }

    // Pattern not cached, so allocate and compile.
    let pattern_string = pattern_str.to_string();
    match regex::Regex::new(&pattern_string) {
        Ok(regex) => REGEX_STATE.with(|state| {
            let mut s = state.borrow_mut();
            let handle = s.slab.len() as i64;
            s.slab.push(regex);
            s.pattern_cache.insert(pattern_string, handle);
            handle
        }),
        Err(e) => {
            // Security invariant: extern "C" FFI functions must never panic across the ABI boundary.
            // Use trap to report the fault and exit cleanly.
            crate::trap::trap(
                crate::trap::code::EXPLICIT_PANIC,
                &format!(
                    "Regex pattern validation mismatch: compiler and runtime regex versions disagree. \
                     Pattern '{}' compiled successfully at type-check time but failed at runtime: {}",
                    pattern_str, e
                ),
            );
        }
    }
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
    guard::guard_check(text as *mut u8);
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
/// If a match is found, its start and end character positions are stored in
/// thread-locals and can be retrieved via `miri_rt_regex_match_start()` and
/// `miri_rt_regex_match_end()`. On invalid handle, returns false.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_find(handle: i64, text: *const MiriString) -> bool {
    guard::guard_check(text as *mut u8);
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
        set_match_in_characters(text, start, end);
        true
    } else {
        false
    }
}

/// Records a match found at byte offsets `[start, end)` of `text` as the
/// character positions `String.substring` and `length()` count in.
///
/// # Safety
/// `text` is a valid `MiriString` pointer or null, and both offsets are
/// character boundaries in it.
unsafe fn set_match_in_characters(text: *const MiriString, start: i64, end: i64) {
    if text.is_null() {
        set_match_result(start, end);
        return;
    }
    let string = &*text;
    let start_char = crate::string::char_index::char_index_of_byte(string, start as usize);
    let end_char = crate::string::char_index::char_index_of_byte(string, end as usize);
    set_match_result(start_char as i64, end_char as i64);
}

/// Finds the first match of the regex starting from a given character position.
///
/// Returns true if a match is found; false otherwise.
/// If a match is found, its start and end character positions are stored in
/// thread-locals and can be retrieved via `miri_rt_regex_match_start()` and
/// `miri_rt_regex_match_end()`. On invalid handle or if `from` is past the
/// text's last character, returns false.
///
/// # Safety
/// - `handle` must be a valid index into the regex slab or -1.
/// - `text` must be a valid `MiriString` pointer or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_regex_find_from(
    handle: i64,
    text: *const MiriString,
    from: i64,
) -> bool {
    guard::guard_check(text as *mut u8);
    let text_str = if text.is_null() { "" } else { (*text).as_str() };

    if from < 0 {
        return false;
    }
    let start_idx = if text.is_null() {
        0
    } else {
        match crate::string::char_index::char_boundary(&*text, from as usize) {
            Some(offset) => offset,
            None => return false,
        }
    };

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
        set_match_in_characters(text, start, end);
        true
    } else {
        false
    }
}

/// Returns the start character position of the last match found by `miri_rt_regex_find()`.
#[no_mangle]
pub extern "C" fn miri_rt_regex_match_start() -> i64 {
    REGEX_STATE.with(|state| state.borrow().match_result.0)
}

/// Returns the end character position of the last match found by `miri_rt_regex_find()`.
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
    guard::guard_check(text as *mut u8);
    guard::guard_check(replacement as *mut u8);
    if handle < 0 {
        let text_str = if text.is_null() { "" } else { (*text).as_str() };
        return crate::string::new_string(text_str);
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
            return crate::string::new_string(text_str);
        }
        let result = s.slab[handle as usize]
            .replace_all(text_str, regex::NoExpand(repl_str))
            .into_owned();
        crate::string::new_string(&result)
    })
}

pub mod ffi {
    pub use super::{
        miri_rt_regex_compile, miri_rt_regex_compile_message, miri_rt_regex_compile_status,
        miri_rt_regex_find, miri_rt_regex_find_from, miri_rt_regex_from_validated_pattern,
        miri_rt_regex_match_end, miri_rt_regex_match_start, miri_rt_regex_matches,
        miri_rt_regex_replace,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rc::balance_guard;

    #[test]
    fn validated_pattern_compiles_valid_regex() {
        let _balance = balance_guard();
        unsafe {
            let pat = MiriString::from_str("[a-z]+");
            let pat_ptr = crate::string::into_raw_ptr(pat);
            let handle = miri_rt_regex_from_validated_pattern(pat_ptr);
            assert!(handle >= 0);

            let text = MiriString::from_str("hello");
            let text_ptr = crate::string::into_raw_ptr(text);
            assert!(miri_rt_regex_matches(handle, text_ptr));

            crate::string::miri_rt_string_free(text_ptr);
            crate::string::miri_rt_string_free(pat_ptr);
        }
    }
}
