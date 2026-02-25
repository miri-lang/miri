//! Memory allocation primitives for the Miri runtime.
//!
//! Wraps Rust's global allocator behind a C-compatible FFI interface.
//! All functions return null on failure rather than panicking, making them
//! safe to call from compiled Miri code across the FFI boundary.

use std::alloc::{self, Layout};
use std::ptr;

/// Allocates `size` bytes of memory with the given alignment.
///
/// Returns a null pointer if `size` is 0, `align` is invalid, or allocation fails.
///
/// # Safety
/// - `align` must be a power of two.
/// - `size`, when rounded up to `align`, must not overflow `isize::MAX`.
#[no_mangle]
pub unsafe extern "C" fn miri_alloc(size: usize, align: usize) -> *mut u8 {
    let layout = match make_layout(size, align) {
        Some(layout) => layout,
        None => return ptr::null_mut(),
    };
    // SAFETY: `layout` has non-zero size (guaranteed by `make_layout`).
    alloc::alloc(layout)
}

/// Allocates `size` bytes of zeroed memory with the given alignment.
///
/// Returns a null pointer if `size` is 0, `align` is invalid, or allocation fails.
///
/// # Safety
/// - `align` must be a power of two.
/// - `size`, when rounded up to `align`, must not overflow `isize::MAX`.
#[no_mangle]
pub unsafe extern "C" fn miri_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    let layout = match make_layout(size, align) {
        Some(layout) => layout,
        None => return ptr::null_mut(),
    };
    // SAFETY: `layout` has non-zero size (guaranteed by `make_layout`).
    alloc::alloc_zeroed(layout)
}

/// Reallocates memory previously allocated by [`miri_alloc`] or [`miri_alloc_zeroed`].
///
/// - If `ptr` is null, behaves like [`miri_alloc`].
/// - If `new_size` is 0, frees the memory and returns null.
///
/// # Safety
/// - `ptr` must have been allocated by [`miri_alloc`] or [`miri_alloc_zeroed`], or be null.
/// - `old_size` and `align` must match the original allocation parameters.
/// - `new_size`, when rounded up to `align`, must not overflow `isize::MAX`.
#[no_mangle]
pub unsafe extern "C" fn miri_realloc(
    ptr: *mut u8,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return miri_alloc(new_size, align);
    }

    if new_size == 0 {
        miri_free(ptr, old_size, align);
        return ptr::null_mut();
    }

    let layout = match make_layout(old_size, align) {
        Some(layout) => layout,
        None => return ptr::null_mut(),
    };

    // SAFETY: `ptr` is non-null and was allocated with `layout`. `new_size` is non-zero.
    alloc::realloc(ptr, layout, new_size)
}

/// Frees memory previously allocated by [`miri_alloc`] or [`miri_alloc_zeroed`].
///
/// No-op if `ptr` is null or `size` is 0. The pointer must not be used after this call.
///
/// # Safety
/// - `ptr` must have been allocated by [`miri_alloc`] or [`miri_alloc_zeroed`], or be null.
/// - `size` and `align` must match the original allocation parameters.
/// - The pointer must not have been freed already (double-free is UB).
#[no_mangle]
pub unsafe extern "C" fn miri_free(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }

    let layout = match make_layout(size, align) {
        Some(layout) => layout,
        None => return,
    };

    // SAFETY: `ptr` is non-null, `layout` matches the original allocation.
    alloc::dealloc(ptr, layout);
}

/// Constructs a [`Layout`] from `size` and `align`, returning `None` for
/// zero-sized or invalid layouts.
#[inline]
fn make_layout(size: usize, align: usize) -> Option<Layout> {
    if size == 0 {
        return None;
    }
    Layout::from_size_align(size, align).ok()
}
