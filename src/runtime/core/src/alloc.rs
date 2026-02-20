//! Memory allocation primitives for Miri runtime.
//!
//! Provides C-compatible allocation functions that wrap Rust's global allocator.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

/// Allocates `size` bytes of memory with the given alignment.
///
/// Returns a null pointer if allocation fails or if size is 0.
///
/// # Safety
/// - The caller must ensure that `align` is a power of two.
/// - The caller must ensure that `size` does not overflow `isize::MAX`.
#[no_mangle]
pub unsafe extern "C" fn miri_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(layout) => layout,
        Err(_) => return ptr::null_mut(),
    };

    // SAFETY: We checked that size is not 0 and layout creation succeeded.
    alloc(layout)
}

/// Allocates `size` bytes of zeroed memory with the given alignment.
///
/// Returns a null pointer if allocation fails or if size is 0.
///
/// # Safety
/// - The caller must ensure that `align` is a power of two.
/// - The caller must ensure that `size` does not overflow `isize::MAX`.
#[no_mangle]
pub unsafe extern "C" fn miri_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(layout) => layout,
        Err(_) => return ptr::null_mut(),
    };

    // SAFETY: We checked that size is not 0 and layout creation succeeded.
    std::alloc::alloc_zeroed(layout)
}

/// Reallocates memory to a new size.
///
/// # Safety
/// - `ptr` must have been allocated by `miri_alloc` or `miri_alloc_zeroed`.
/// - `old_size` and `align` must match the original allocation.
/// - `new_size` must be greater than 0.
/// - The caller must ensure that `new_size` does not overflow `isize::MAX`.
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

    let layout = match Layout::from_size_align(old_size, align) {
        Ok(layout) => layout,
        Err(_) => return ptr::null_mut(),
    };

    // SAFETY:
    // - `ptr` is non-null and was allocated with `layout`.
    // - `new_size` is non-zero (handled above).
    // - `layout.size()` > 0 is implied by `old_size` > 0 if `ptr` is not null (assuming valid usage).
    realloc(ptr, layout, new_size)
}

/// Frees memory previously allocated by `miri_alloc`.
///
/// # Safety
/// - `ptr` must have been allocated by `miri_alloc` or `miri_alloc_zeroed`.
/// - `size` and `align` must match the original allocation.
/// - The pointer must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn miri_free(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(layout) => layout,
        Err(_) => return,
    };

    // SAFETY:
    // - `ptr` is non-null.
    // - `layout` matches the allocation.
    dealloc(ptr, layout);
}
