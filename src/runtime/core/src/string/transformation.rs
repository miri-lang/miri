//! FFI functions that produce new `MiriString` values from existing ones.
//!
//! Every function in this module allocates and returns a fresh `MiriString`.
//! The caller is responsible for freeing the returned pointer via
//! [`super::miri_rt_string_free`].

use super::{into_raw_ptr, miri_rt_string_new, MiriString};

/// Concatenates two strings and returns a new string.
///
/// Handles null pointers gracefully — a null operand is treated as empty.
/// Returns an empty string on integer overflow or allocation failure.
///
/// # Safety
/// - Both pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_concat(
    left: *const MiriString,
    right: *const MiriString,
) -> *mut MiriString {
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
pub unsafe extern "C" fn miri_rt_string_to_lower(ptr: *const MiriString) -> *mut MiriString {
    transform_str(ptr, |s| s.to_lowercase())
}

/// Converts a string to uppercase (Unicode-aware).
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_to_upper(ptr: *const MiriString) -> *mut MiriString {
    transform_str(ptr, |s| s.to_uppercase())
}

/// Trims whitespace from both ends of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim(ptr: *const MiriString) -> *mut MiriString {
    transform_str_ref(ptr, str::trim)
}

/// Trims whitespace from the start (left side) of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim_start(ptr: *const MiriString) -> *mut MiriString {
    transform_str_ref(ptr, str::trim_start)
}

/// Trims whitespace from the end (right side) of a string.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim_end(ptr: *const MiriString) -> *mut MiriString {
    transform_str_ref(ptr, str::trim_end)
}

/// Replaces all occurrences of `from` with `to` in the string.
///
/// If `from` is empty or null, returns a copy of the original string.
///
/// # Safety
/// - All pointers must be valid `MiriString` pointers with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_replace(
    s: *const MiriString,
    from: *const MiriString,
    to: *const MiriString,
) -> *mut MiriString {
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
pub unsafe extern "C" fn miri_rt_string_substring(
    s: *const MiriString,
    start: usize,
    end: usize,
) -> *mut MiriString {
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
pub unsafe extern "C" fn miri_rt_string_char_at(
    ptr: *const MiriString,
    index: usize,
) -> *mut MiriString {
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
/// Returns an empty string if `ptr` is null or `count` is 0.
///
/// # Safety
/// - `ptr` must be a valid `MiriString` pointer with valid UTF-8, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_repeat(
    ptr: *const MiriString,
    count: usize,
) -> *mut MiriString {
    if ptr.is_null() || count == 0 {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let repeated = s.repeat(count);
    into_raw_ptr(MiriString::from_str(&repeated))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

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
