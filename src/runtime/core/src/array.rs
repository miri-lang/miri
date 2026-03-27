//! Fixed-size array implementation for Miri runtime.
//!
//! Unlike `MiriList`, a `MiriArray` has a fixed element count determined at
//! creation time. The backing buffer is allocated once and never grows.
//! All elements are zeroed at creation.
//!
//! Memory layout (allocated by FFI functions):
//! `[RC][data|elem_count|elem_size]` — the pointer returned to the compiler
//! points past the RC header, so field offsets are unchanged.

use std::alloc::{alloc_zeroed, dealloc, Layout};

use crate::rc::{alloc_with_rc, free_with_rc};

/// A type-erased fixed-size array.
///
/// Stores elements as contiguous bytes. The element count is fixed at
/// creation and never changes.
///
/// Memory layout matches what Miri expects:
/// - `data`: Pointer to element storage
/// - `elem_count`: Number of elements (fixed at creation)
/// - `elem_size`: Size of each element in bytes
#[repr(C)]
pub struct MiriArray {
    data: *mut u8,
    elem_count: usize,
    elem_size: usize,
}

const STRUCT_SIZE: usize = std::mem::size_of::<MiriArray>();

impl MiriArray {
    fn byte_len(&self) -> usize {
        self.elem_count
            .checked_mul(self.elem_size)
            .unwrap_or_else(|| std::process::abort())
    }

    /// Returns the raw data pointer.
    pub fn data_ptr(&self) -> *const u8 {
        self.data
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.elem_count
    }

    /// Returns `true` if the array contains no elements.
    pub fn is_empty(&self) -> bool {
        self.elem_count == 0
    }

    /// Returns the size of each element in bytes.
    pub fn elem_size(&self) -> usize {
        self.elem_size
    }
}

// Drop impl is only used by Rust-side unit tests (which create MiriArray
// via Box::new). FFI allocations use alloc_with_rc and are freed manually.
impl Drop for MiriArray {
    fn drop(&mut self) {
        if !self.data.is_null() && self.elem_count > 0 && self.elem_size > 0 {
            if let Ok(layout) = Layout::from_size_align(self.byte_len(), 8) {
                unsafe {
                    dealloc(self.data, layout);
                }
            }
        }
    }
}

// =============================================================================
// FFI Functions
// =============================================================================

/// Stable FFI interface for array operations.
pub mod ffi {
    use super::*;
    use crate::list::MiriList;
    use std::ptr;

    /// Creates a new fixed-size array with the given element count and size.
    ///
    /// Allocates `[RC=1][MiriArray fields]`. All data elements are zeroed.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_new(
        elem_count: usize,
        elem_size: usize,
    ) -> *mut MiriArray {
        let payload = alloc_with_rc(STRUCT_SIZE);
        if payload.is_null() {
            return ptr::null_mut();
        }

        let arr = payload as *mut MiriArray;

        if elem_count > 0 && elem_size > 0 {
            let total = match elem_count.checked_mul(elem_size) {
                Some(t) => t,
                None => {
                    (*arr).data = ptr::null_mut();
                    (*arr).elem_count = 0;
                    (*arr).elem_size = elem_size;
                    return arr;
                }
            };
            let layout = match Layout::from_size_align(total, 8) {
                Ok(l) => l,
                Err(_) => {
                    (*arr).data = ptr::null_mut();
                    (*arr).elem_count = 0;
                    (*arr).elem_size = elem_size;
                    return arr;
                }
            };
            let data = alloc_zeroed(layout);
            (*arr).data = data;
            (*arr).elem_count = if data.is_null() { 0 } else { elem_count };
        } else {
            (*arr).data = ptr::null_mut();
            (*arr).elem_count = 0;
        }
        (*arr).elem_size = elem_size;

        arr
    }

    /// Frees the array and its backing storage.
    ///
    /// The pointer must have been returned by `miri_rt_array_new` (i.e., it
    /// points past the RC header).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_free(ptr: *mut MiriArray) {
        if ptr.is_null() {
            return;
        }
        // Free internal data buffer
        let arr = &*ptr;
        if !arr.data.is_null() && arr.elem_count > 0 && arr.elem_size > 0 {
            let layout = Layout::from_size_align(arr.byte_len(), 8)
                .unwrap_or_else(|_| std::process::abort());
            dealloc(arr.data, layout);
        }
        // Free the [RC][struct] block
        free_with_rc(ptr as *mut u8, STRUCT_SIZE);
    }

    /// Returns the number of elements in the array (fixed at creation).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_len(ptr: *const MiriArray) -> usize {
        if ptr.is_null() {
            return 0;
        }
        (*ptr).elem_count
    }

    /// Gets a pointer to the element at the given index.
    ///
    /// Returns null if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_get(ptr: *const MiriArray, index: usize) -> *const u8 {
        if ptr.is_null() {
            return ptr::null();
        }
        let arr = &*ptr;
        if index >= arr.elem_count || arr.data.is_null() {
            return ptr::null();
        }
        arr.data.add(index * arr.elem_size)
    }

    /// Gets a mutable pointer to the element at the given index.
    ///
    /// Returns null if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_get_mut(ptr: *mut MiriArray, index: usize) -> *mut u8 {
        if ptr.is_null() {
            return ptr::null_mut();
        }
        let arr = &*ptr;
        if index >= arr.elem_count || arr.data.is_null() {
            return ptr::null_mut();
        }
        arr.data.add(index * arr.elem_size)
    }

    /// Sets the element at the given index.
    ///
    /// Returns true (1) if successful, false (0) if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_set(
        ptr: *mut MiriArray,
        index: usize,
        elem: *const u8,
    ) -> u8 {
        if ptr.is_null() || elem.is_null() {
            return 0;
        }
        let arr = &*ptr;
        if index >= arr.elem_count || arr.data.is_null() {
            return 0;
        }
        let dest = arr.data.add(index * arr.elem_size);
        ptr::copy_nonoverlapping(elem, dest, arr.elem_size);
        1
    }

    /// Sets the element at the given index, passing the value by value (as usize).
    ///
    /// The value is copied from the address of `val` on the caller's stack,
    /// so this works for any element type that fits in a pointer-sized register.
    /// This is the value-based variant used by the stdlib `set` method, which
    /// receives the element as a Miri value (not a raw pointer).
    ///
    /// Returns true (1) if successful, false (0) if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_set_val(
        ptr: *mut MiriArray,
        index: usize,
        val: usize,
    ) -> u8 {
        miri_rt_array_set(ptr, index, &val as *const usize as *const u8)
    }

    /// Fills all elements with the given value.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_fill(ptr: *mut MiriArray, elem: *const u8) {
        if ptr.is_null() || elem.is_null() {
            return;
        }
        let arr = &*ptr;
        if arr.data.is_null() {
            return;
        }
        for i in 0..arr.elem_count {
            let dest = arr.data.add(i * arr.elem_size);
            ptr::copy_nonoverlapping(elem, dest, arr.elem_size);
        }
    }

    /// Returns a clone of the array.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_clone(ptr: *const MiriArray) -> *mut MiriArray {
        if ptr.is_null() {
            return miri_rt_array_new(0, 0);
        }
        let src = &*ptr;
        let new_arr = miri_rt_array_new(src.elem_count, src.elem_size);
        if !new_arr.is_null() && !src.data.is_null() && !(*new_arr).data.is_null() {
            ptr::copy_nonoverlapping(src.data, (*new_arr).data, src.byte_len());
        }
        new_arr
    }

    /// Sorts the array in ascending order (elements compared as signed 64-bit integers).
    ///
    /// Uses insertion sort which is stable and efficient for small arrays.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_sort(ptr: *mut MiriArray) {
        if ptr.is_null() {
            return;
        }
        let arr = &*ptr;
        if arr.elem_count < 2 || arr.data.is_null() {
            return;
        }

        let elem_size = arr.elem_size;
        let mut temp = vec![0u8; elem_size];

        for i in 1..arr.elem_count {
            let src = arr.data.add(i * elem_size);
            ptr::copy_nonoverlapping(src, temp.as_mut_ptr(), elem_size);
            let key = crate::list::read_as_i64(temp.as_ptr(), elem_size);

            let mut j = i;
            while j > 0 {
                let prev = arr.data.add((j - 1) * elem_size);
                let prev_val = crate::list::read_as_i64(prev, elem_size);
                if prev_val <= key {
                    break;
                }
                let dest = arr.data.add(j * elem_size);
                ptr::copy_nonoverlapping(prev, dest, elem_size);
                j -= 1;
            }
            let dest = arr.data.add(j * elem_size);
            ptr::copy_nonoverlapping(temp.as_ptr(), dest, elem_size);
        }
    }

    /// Returns a raw pointer to the underlying data buffer.
    ///
    /// The pointer is valid for `elem_count * elem_size` bytes.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_data(ptr: *const MiriArray) -> *const u8 {
        if ptr.is_null() {
            return ptr::null();
        }
        (*ptr).data
    }

    /// Converts the array to a MiriList containing all elements.
    ///
    /// The caller owns the returned list and must free it with `miri_rt_list_free`.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_to_list(ptr: *const MiriArray) -> *mut MiriList {
        if ptr.is_null() {
            return crate::miri_rt_list_new(0);
        }
        let arr = &*ptr;
        let list = crate::miri_rt_list_new(arr.elem_size);
        if arr.data.is_null() {
            return list;
        }
        for i in 0..arr.elem_count {
            let src = arr.data.add(i * arr.elem_size);
            (*list).push(src);
        }
        list
    }

    // =============================================================================
    // Error Handling
    // =============================================================================

    /// Panics with a clear out-of-bounds error message.
    ///
    /// This provides a better debugging experience than crashing silently on
    /// a hardware trap.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_panic_oob(index: usize, len: usize) {
        eprintln!(
            "Runtime error: Array index out of bounds: the len is {} but the index is {}",
            len, index
        );
        std::process::abort();
    }
} // pub mod ffi

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::ffi::*;

    #[test]
    fn test_array_new_zeroed() {
        unsafe {
            let arr = miri_rt_array_new(5, std::mem::size_of::<i32>());
            assert_eq!(miri_rt_array_len(arr), 5);

            // All elements should be zero
            for i in 0..5 {
                let p = miri_rt_array_get(arr, i);
                assert!(!p.is_null());
                assert_eq!(*(p as *const i32), 0);
            }

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_get_set() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

            let val = 42i32;
            assert_eq!(
                miri_rt_array_set(arr, 1, &val as *const i32 as *const u8),
                1
            );

            let p = miri_rt_array_get(arr, 1);
            assert_eq!(*(p as *const i32), 42);

            // Other elements still zero
            assert_eq!(*(miri_rt_array_get(arr, 0) as *const i32), 0);
            assert_eq!(*(miri_rt_array_get(arr, 2) as *const i32), 0);

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_bounds_checking() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

            // Out of bounds get returns null
            assert!(miri_rt_array_get(arr, 3).is_null());
            assert!(miri_rt_array_get(arr, 100).is_null());

            // Out of bounds set returns 0
            let val = 1i32;
            assert_eq!(
                miri_rt_array_set(arr, 3, &val as *const i32 as *const u8),
                0
            );

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_fill() {
        unsafe {
            let arr = miri_rt_array_new(4, std::mem::size_of::<i32>());

            let val = 99i32;
            miri_rt_array_fill(arr, &val as *const i32 as *const u8);

            for i in 0..4 {
                let p = miri_rt_array_get(arr, i);
                assert_eq!(*(p as *const i32), 99);
            }

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_clone() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

            let values = [10i32, 20, 30];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i32 as *const u8);
            }

            let cloned = miri_rt_array_clone(arr);
            assert_eq!(miri_rt_array_len(cloned), 3);

            for (i, v) in values.iter().enumerate() {
                let p = miri_rt_array_get(cloned, i);
                assert_eq!(*(p as *const i32), *v);
            }

            // Modifying original doesn't affect clone
            let new_val = 999i32;
            miri_rt_array_set(arr, 0, &new_val as *const i32 as *const u8);
            assert_eq!(*(miri_rt_array_get(cloned, 0) as *const i32), 10);

            miri_rt_array_free(arr);
            miri_rt_array_free(cloned);
        }
    }

    #[test]
    fn test_array_to_list() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

            let values = [5i32, 10, 15];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i32 as *const u8);
            }

            let list = miri_rt_array_to_list(arr);
            assert_eq!(crate::miri_rt_list_len(list), 3);

            for (i, v) in values.iter().enumerate() {
                let p = crate::miri_rt_list_get(list, i);
                assert_eq!(*(p as *const i32), *v);
            }

            crate::miri_rt_list_free(list);
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_data_ptr() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

            let val = 7i32;
            miri_rt_array_set(arr, 0, &val as *const i32 as *const u8);

            let data = miri_rt_array_data(arr);
            assert!(!data.is_null());
            assert_eq!(*(data as *const i32), 7);

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_empty() {
        unsafe {
            let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
            assert_eq!(miri_rt_array_len(arr), 0);
            assert!(miri_rt_array_get(arr, 0).is_null());
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort() {
        unsafe {
            let arr = miri_rt_array_new(4, std::mem::size_of::<i64>());

            let values = [30i64, 10, 20, 5];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i64 as *const u8);
            }

            miri_rt_array_sort(arr);

            assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 5);
            assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), 10);
            assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 20);
            assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 30);

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_rc_header_present() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
            assert!(!arr.is_null());

            let rc_ptr = (arr as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *const usize;
            assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_null_safety() {
        unsafe {
            assert_eq!(miri_rt_array_len(std::ptr::null()), 0);
            assert!(miri_rt_array_get(std::ptr::null(), 0).is_null());
            assert!(miri_rt_array_get_mut(std::ptr::null_mut(), 0).is_null());
            assert_eq!(
                miri_rt_array_set(std::ptr::null_mut(), 0, std::ptr::null()),
                0
            );
            miri_rt_array_fill(std::ptr::null_mut(), std::ptr::null()); // must not crash
            assert!(miri_rt_array_data(std::ptr::null()).is_null());
            miri_rt_array_sort(std::ptr::null_mut()); // must not crash
            miri_rt_array_free(std::ptr::null_mut()); // must not crash
        }
    }

    #[test]
    fn test_array_set_null_elem() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
            assert_eq!(miri_rt_array_set(arr, 0, std::ptr::null()), 0);
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort_negative_values() {
        unsafe {
            let arr = miri_rt_array_new(5, std::mem::size_of::<i64>());
            let values = [-10i64, 5, -3, 0, 7];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i64 as *const u8);
            }

            miri_rt_array_sort(arr);

            assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), -10);
            assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), -3);
            assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 0);
            assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 5);
            assert_eq!(*(miri_rt_array_get(arr, 4) as *const i64), 7);

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort_duplicates() {
        unsafe {
            let arr = miri_rt_array_new(5, std::mem::size_of::<i64>());
            let values = [3i64, 1, 3, 2, 1];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i64 as *const u8);
            }

            miri_rt_array_sort(arr);

            assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 1);
            assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), 1);
            assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 2);
            assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 3);
            assert_eq!(*(miri_rt_array_get(arr, 4) as *const i64), 3);

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort_reverse_sorted() {
        unsafe {
            let arr = miri_rt_array_new(4, std::mem::size_of::<i64>());
            let values = [4i64, 3, 2, 1];
            for (i, v) in values.iter().enumerate() {
                miri_rt_array_set(arr, i, v as *const i64 as *const u8);
            }

            miri_rt_array_sort(arr);

            for i in 0..4 {
                assert_eq!(*(miri_rt_array_get(arr, i) as *const i64), (i + 1) as i64);
            }

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort_single_element() {
        unsafe {
            let arr = miri_rt_array_new(1, std::mem::size_of::<i64>());
            let val = 42i64;
            miri_rt_array_set(arr, 0, &val as *const i64 as *const u8);
            miri_rt_array_sort(arr); // must not crash
            assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 42);
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_sort_empty() {
        unsafe {
            let arr = miri_rt_array_new(0, std::mem::size_of::<i64>());
            miri_rt_array_sort(arr); // must not crash
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_clone_empty() {
        unsafe {
            let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
            let cloned = miri_rt_array_clone(arr);
            assert_eq!(miri_rt_array_len(cloned), 0);
            miri_rt_array_free(arr);
            miri_rt_array_free(cloned);
        }
    }

    #[test]
    fn test_array_clone_null() {
        unsafe {
            let cloned = miri_rt_array_clone(std::ptr::null());
            assert!(!cloned.is_null());
            assert_eq!(miri_rt_array_len(cloned), 0);
            miri_rt_array_free(cloned);
        }
    }

    #[test]
    fn test_array_get_mut() {
        unsafe {
            let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
            let val = 77i32;
            miri_rt_array_set(arr, 1, &val as *const i32 as *const u8);

            let p = miri_rt_array_get_mut(arr, 1);
            assert!(!p.is_null());
            assert_eq!(*(p as *const i32), 77);

            // Write through mutable pointer
            *(p as *mut i32) = 88;
            assert_eq!(*(miri_rt_array_get(arr, 1) as *const i32), 88);

            // Out of bounds
            assert!(miri_rt_array_get_mut(arr, 3).is_null());

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_fill_all_elements() {
        unsafe {
            let arr = miri_rt_array_new(100, std::mem::size_of::<i64>());
            let val = 42i64;
            miri_rt_array_fill(arr, &val as *const i64 as *const u8);

            for i in 0..100 {
                assert_eq!(*(miri_rt_array_get(arr, i) as *const i64), 42);
            }

            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_to_list_empty() {
        unsafe {
            let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
            let list = miri_rt_array_to_list(arr);
            assert_eq!(crate::miri_rt_list_len(list), 0);
            crate::miri_rt_list_free(list);
            miri_rt_array_free(arr);
        }
    }

    #[test]
    fn test_array_to_list_null() {
        unsafe {
            let list = miri_rt_array_to_list(std::ptr::null());
            assert!(!list.is_null());
            assert_eq!(crate::miri_rt_list_len(list), 0);
            crate::miri_rt_list_free(list);
        }
    }
}
