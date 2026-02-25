//! FFI functions for creating, cloning, and freeing `MiriString` values.

use super::{into_raw_ptr, MiriString};
use std::{slice, str};

/// Creates a new empty string.
///
/// Returns a pointer to a heap-allocated `MiriString` with no data.
#[no_mangle]
pub extern "C" fn miri_rt_string_new() -> *mut MiriString {
    into_raw_ptr(MiriString::new())
}

/// Creates a string from raw UTF-8 bytes.
///
/// The `data` buffer is **copied** — the caller retains ownership of the original.
/// Returns an empty string if `data` is null, `len` is zero, or the bytes are
/// not valid UTF-8.
///
/// # Safety
/// - If `data` is non-null, it must point to at least `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_from_raw(data: *const u8, len: usize) -> *mut MiriString {
    if data.is_null() || len == 0 {
        return miri_rt_string_new();
    }

    // SAFETY: Caller guarantees `data` points to `len` readable bytes.
    let bytes = slice::from_raw_parts(data, len);
    let s = match str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return miri_rt_string_new(),
    };

    into_raw_ptr(MiriString::from_str(s))
}

/// Frees a `MiriString` previously allocated by this runtime.
///
/// No-op if `ptr` is null. The pointer must not be used after this call.
///
/// # Safety
/// - `ptr` must have been returned by a `miri_rt_string_*` constructor, or be null.
/// - The pointer must not have been freed already (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_free(ptr: *mut MiriString) {
    if !ptr.is_null() {
        // SAFETY: `ptr` was created via `Box::into_raw` in a constructor.
        // `Box::from_raw` reclaims ownership; `MiriString::drop` frees the data buffer.
        let _ = Box::from_raw(ptr);
    }
}

/// Creates a deep copy of a `MiriString`.
///
/// Returns an empty string if `ptr` is null.
///
/// # Safety
/// - `ptr` must be a valid pointer to a live `MiriString`, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_clone(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    // SAFETY: Caller guarantees `ptr` is valid and contains UTF-8.
    let s = (*ptr).as_str();
    into_raw_ptr(MiriString::from_str(s))
}
