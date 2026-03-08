//! Reference counting header utilities for heap-allocated Miri values.
//!
//! All heap-allocated types (strings, arrays, lists, user classes) share
//! the same memory layout: `[RC][payload]`. The variable holds a pointer
//! to the payload; the RC is at `ptr - RC_HEADER_SIZE`.
//!
//! This module provides helpers for allocation and deallocation with
//! this layout, so every heap type uses the same convention.

use std::alloc::{alloc_zeroed, dealloc, Layout};

/// Size of the reference count header, in bytes.
/// Matches `ptr_type.bytes()` in the Cranelift codegen.
pub const RC_HEADER_SIZE: usize = std::mem::size_of::<usize>();

/// Allocates `[RC=1][payload]` and returns a pointer to the payload.
///
/// The payload is zeroed. The caller can write struct fields into the
/// returned pointer. To free, use [`free_with_rc`].
///
/// Returns null if allocation fails.
pub unsafe fn alloc_with_rc(payload_size: usize) -> *mut u8 {
    let total_size = RC_HEADER_SIZE + payload_size;
    let layout = match Layout::from_size_align(total_size, 8) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };

    let base = alloc_zeroed(layout);
    if base.is_null() {
        return std::ptr::null_mut();
    }

    // Set RC = 1
    *(base as *mut usize) = 1;

    // Return pointer past RC header (to the payload)
    base.add(RC_HEADER_SIZE)
}

/// Frees the `[RC][payload]` block given a pointer to the payload.
///
/// The caller must have already cleaned up any resources owned by
/// the payload (e.g., freeing a data buffer inside a MiriArray).
pub unsafe fn free_with_rc(payload_ptr: *mut u8, payload_size: usize) {
    if payload_ptr.is_null() {
        return;
    }
    let base = payload_ptr.sub(RC_HEADER_SIZE);
    let total_size = RC_HEADER_SIZE + payload_size;
    let layout = Layout::from_size_align(total_size, 8).unwrap_or_else(|_| std::process::abort());
    dealloc(base, layout);
}
