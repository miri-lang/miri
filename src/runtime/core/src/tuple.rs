// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tuple support for Miri runtime.
//!
//! Tuples are heap-allocated by the Cranelift backend with the layout:
//! `[size][RC][elem_count][field0][field1]...`
//!
//! The pointer stored in a Miri variable points to the payload, i.e. past the
//! `[size][RC]` header, so `elem_count` is at offset 0.

use crate::guard;

/// Stable FFI interface for tuple operations.
pub mod ffi {
    use super::*;

    /// Returns the number of elements in a tuple.
    ///
    /// The pointer must be a valid Miri tuple payload pointer (past the
    /// `[size][RC]` header). Returns 0 for null pointers.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_tuple_len(ptr: *const usize) -> usize {
        if ptr.is_null() {
            return 0;
        }
        // Defense in depth: validate managed payload pointer on FFI entry under MIRI_HEAP_GUARD
        // to detect use-after-free or header corruption before dereferencing `ptr`.
        guard::guard_check(ptr as *mut u8);
        *ptr
    }
}

#[cfg(test)]
mod tests {
    use super::ffi::miri_rt_tuple_len;

    #[test]
    fn tuple_len_null_returns_zero() {
        unsafe {
            assert_eq!(miri_rt_tuple_len(std::ptr::null()), 0);
        }
    }

    #[test]
    fn tuple_len_reads_elem_count() {
        let count: usize = 42;
        unsafe {
            assert_eq!(miri_rt_tuple_len(&count as *const usize), 42);
        }
    }
}
