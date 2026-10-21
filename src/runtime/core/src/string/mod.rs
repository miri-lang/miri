// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! String implementation for the Miri runtime.
//!
//! Provides a UTF-8 string type (`MiriString`) with a C-compatible FFI interface.
//! All FFI functions are `#[no_mangle] extern "C"` and null-safe.
//!
//! # Module Organization
//! - [`core`] — `MiriString` type definition, inherent methods, and `Drop`/`Default` impls.
//! - [`char_index`] — Character positions found without a rescan per step.
//! - [`constructors`] — FFI functions for creating, cloning, and freeing strings.
//! - [`inspection`] — FFI functions for querying string properties (length, contains, etc.).
//! - [`transformation`] — FFI functions that produce new strings (concat, trim, replace, etc.).
//! - [`conversion`] — FFI functions for converting primitive types to strings.

pub(crate) mod char_index;
mod constructors;
mod conversion;
mod core;
mod format_spec;
mod inspection;
mod operations;
mod pieces;
mod transformation;

pub use core::{MiriString, StringStorage};

// Re-export all FFI functions at module level for backward-compatible access
// via `miri_runtime_core::string::miri_rt_string_*`.
pub use ffi::*;

/// The most bytes a string keeps in its header's own block.
///
/// The block holds the RC word, the header and the bytes, and this is the
/// room left in the largest block the per-thread block cache keeps: 216 bytes
/// on a 64-bit target (256 - 8 - 32), 236 on a 32-bit one (256 - 4 - 16).
/// Every inline string therefore comes from, and goes back to, the cache. It
/// is stored in a header's `block_room` byte, so it must fit a `u8`.
pub const INLINE_CAPACITY: usize = crate::block_cache::LARGEST_CACHED_BLOCK
    - crate::rc::RC_HEADER_SIZE
    - std::mem::size_of::<MiriString>();

const _: () = assert!(INLINE_CAPACITY <= u8::MAX as usize);

/// Heap-allocates a [`MiriString`] into an RC block (`[RC=1][payload]`) and
/// returns a pointer to the payload.
///
/// The caller (Miri compiled code) is responsible for eventually freeing
/// this pointer via [`miri_rt_string_free`].
///
/// Returns null if the RC allocation fails (in which case `s` is dropped
/// in place, freeing its data buffer).
#[inline]
pub(crate) fn into_raw_ptr(s: MiriString) -> *mut MiriString {
    let payload_size = std::mem::size_of::<MiriString>();
    // SAFETY: payload_size is the exact size of MiriString.
    let ptr = unsafe { crate::rc::alloc_with_rc(payload_size) as *mut MiriString };
    if ptr.is_null() {
        // Allocation failed; `s` drops here, freeing its data buffer.
        return std::ptr::null_mut();
    }
    // Move `s` into the RC block without running its Drop impl.
    // SAFETY: `ptr` is non-null, aligned, and points to `payload_size` writable
    // bytes; this write sets every field.
    unsafe { std::ptr::write(ptr, s) };
    ptr
}

/// A new string holding `text`, with RC = 1: inline in its header's block
/// when it fits [`INLINE_CAPACITY`], in a heap buffer otherwise.
///
/// Returns null if an allocation fails.
pub(crate) fn new_string(text: &str) -> *mut MiriString {
    new_string_of_ascii(text, text.is_ascii())
}

/// As [`new_string`], for text already known to be ASCII or not, so it is not
/// scanned again.
pub(crate) fn new_string_of_ascii(text: &str, ascii: bool) -> *mut MiriString {
    // SAFETY: the string has room for `text.len()` bytes, and `text` lies
    // outside the block just allocated.
    unsafe {
        let string = alloc_string(text.len(), ascii);
        if !string.is_null() && !text.is_empty() {
            std::ptr::copy_nonoverlapping(text.as_ptr(), (*string).data, text.len());
        }
        string
    }
}

/// A new string of `len` bytes, with RC = 1, whose bytes are left for the
/// caller to write: inline when `len` fits [`INLINE_CAPACITY`], in a heap
/// buffer otherwise, and with no buffer at all when `len` is zero.
///
/// Returns null if an allocation fails.
///
/// # Safety
/// The caller writes all `len` bytes, as valid UTF-8 that is ASCII exactly
/// when `ascii` says so, before the string is read.
pub(crate) unsafe fn alloc_string(len: usize, ascii: bool) -> *mut MiriString {
    if len == 0 {
        return into_raw_ptr(MiriString::new());
    }
    if len <= INLINE_CAPACITY {
        return alloc_inline(len, ascii);
    }
    // SAFETY: `len` is non-zero and alignment 1 is always valid.
    let data = crate::alloc::miri_alloc(len, 1);
    if data.is_null() {
        return std::ptr::null_mut();
    }
    into_raw_ptr(MiriString {
        data,
        len,
        capacity: len,
        ascii,
        storage: StringStorage::Heap,
        block_room: 0,
    })
}

/// Allocates one block holding the RC word, a header and `len` bytes after
/// it, and writes the header in place with `data` pointing at those bytes.
///
/// The header is built in its block rather than moved there: its `data`
/// points into the block, so a header built anywhere else would point at the
/// wrong place once moved.
///
/// # Safety
/// `len` is between 1 and [`INLINE_CAPACITY`]; the caller writes the bytes as
/// for [`alloc_string`].
unsafe fn alloc_inline(len: usize, ascii: bool) -> *mut MiriString {
    debug_assert!((1..=INLINE_CAPACITY).contains(&len));
    let header_size = std::mem::size_of::<MiriString>();
    let string = crate::rc::alloc_with_rc(header_size + len) as *mut MiriString;
    if string.is_null() {
        return string;
    }
    // SAFETY: the block holds `header_size + len` writable bytes, aligned for
    // a header; this write sets every field.
    std::ptr::write(
        string,
        MiriString {
            data: (string as *mut u8).add(header_size),
            len,
            capacity: len,
            ascii,
            storage: StringStorage::Inline,
            block_room: len as u8,
        },
    );
    string
}

/// Converts a Rust `bool` into the FFI representation used by Miri (0 or 1).
#[inline]
const fn bool_to_ffi(value: bool) -> u8 {
    value as u8
}

/// Safely dereferences a `*const MiriString`, returning `""` for null pointers.
///
/// # Safety
/// If `ptr` is non-null it must point to a valid, live `MiriString` whose data
/// is valid UTF-8.
#[inline]
unsafe fn deref_as_str<'a>(ptr: *const MiriString) -> &'a str {
    if ptr.is_null() {
        ""
    } else {
        (*ptr).as_str()
    }
}

/// Stable FFI interface for string operations.
///
/// Aggregates all `#[no_mangle] extern "C"` functions from the string submodules.
pub mod ffi {
    pub use super::constructors::*;
    pub use super::conversion::*;
    pub use super::format_spec::*;
    pub use super::inspection::*;
    pub use super::operations::*;
    pub use super::pieces::*;
    pub use super::transformation::*;
}
