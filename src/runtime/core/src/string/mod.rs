//! String implementation for the Miri runtime.
//!
//! Provides a UTF-8 string type (`MiriString`) with a C-compatible FFI interface.
//! All FFI functions are `#[no_mangle] extern "C"` and null-safe.
//!
//! # Module Organization
//! - [`core`] — `MiriString` type definition, inherent methods, and `Drop`/`Default` impls.
//! - [`constructors`] — FFI functions for creating, cloning, and freeing strings.
//! - [`inspection`] — FFI functions for querying string properties (length, contains, etc.).
//! - [`transformation`] — FFI functions that produce new strings (concat, trim, replace, etc.).
//! - [`conversion`] — FFI functions for converting primitive types to strings.

mod constructors;
mod conversion;
mod core;
mod inspection;
mod transformation;

pub use core::MiriString;

// Re-export all FFI functions at module level for backward-compatible access
// via `miri_runtime_core::string::miri_rt_string_*`.
pub use ffi::*;

// ---------------------------------------------------------------------------
// Internal helpers shared across submodules
// ---------------------------------------------------------------------------

/// Heap-allocates a [`MiriString`] and returns a raw pointer suitable for FFI.
///
/// The caller (Miri compiled code) is responsible for eventually freeing
/// this pointer via [`miri_rt_string_free`].
#[inline]
pub(crate) fn into_raw_ptr(s: MiriString) -> *mut MiriString {
    Box::into_raw(Box::new(s))
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
    pub use super::inspection::*;
    pub use super::transformation::*;
}
