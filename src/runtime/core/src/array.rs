// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
/// - `elem_drop_fn`: If non-zero, called on each element pointer when the
///   array is freed so that managed elements have their RC decremented.
/// - `elem_clone_fn`: If non-zero, called on each element pointer during
///   `miri_rt_array_clone` to produce a deep copy instead of an IncRef.
///   Signature: `fn(*mut u8) -> *mut u8`. Must only be set for user-defined
///   class elements that implement `Cloneable`.
#[repr(C)]
pub struct MiriArray {
    data: *mut u8,
    elem_count: usize,
    elem_size: usize,
    elem_drop_fn: usize,
    elem_clone_fn: usize,
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
                    (*arr).elem_drop_fn = 0;
                    (*arr).elem_clone_fn = 0;
                    return arr;
                }
            };
            let layout = match Layout::from_size_align(total, 8) {
                Ok(l) => l,
                Err(_) => {
                    (*arr).data = ptr::null_mut();
                    (*arr).elem_count = 0;
                    (*arr).elem_size = elem_size;
                    (*arr).elem_drop_fn = 0;
                    (*arr).elem_clone_fn = 0;
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
        (*arr).elem_drop_fn = 0;
        (*arr).elem_clone_fn = 0;

        arr
    }

    /// Frees the array and its backing storage.
    ///
    /// The pointer must have been returned by `miri_rt_array_new` (i.e., it
    /// points past the RC header).
    ///
    /// If `elem_drop_fn` is set, calls it on each element pointer (reading the
    /// slot as a pointer-sized word) before freeing the data buffer, so that
    /// managed elements have their RC decremented.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_free(ptr: *mut MiriArray) {
        if ptr.is_null() {
            return;
        }
        // Free internal data buffer
        let arr = &*ptr;
        if !arr.data.is_null() && arr.elem_count > 0 && arr.elem_size > 0 {
            if arr.elem_drop_fn != 0 {
                let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(arr.elem_drop_fn);
                for i in 0..arr.elem_count {
                    let slot = arr.data.add(i * arr.elem_size) as *const usize;
                    let elem_ptr = *slot;
                    if elem_ptr != 0 {
                        drop_fn(elem_ptr as *mut u8);
                    }
                }
            }
            let layout = Layout::from_size_align(arr.byte_len(), 8)
                .unwrap_or_else(|_| std::process::abort());
            dealloc(arr.data, layout);
        }
        // Free the [RC][struct] block
        free_with_rc(ptr as *mut u8, STRUCT_SIZE);
    }

    /// Decrements the RC of a managed Array element and frees it if RC reaches zero.
    ///
    /// Used as a direct decref callback when an Array slot is overwritten
    /// (e.g., `arr[i] = new_array` where the element type is itself an Array).
    ///
    /// Calls `miri_rt_array_free`, which invokes `elem_drop_fn` on every
    /// non-null element before freeing the backing buffer.  Managed elements
    /// nested inside the array (e.g. `Array<String>` or `Array<List<int>>`)
    /// are therefore correctly DecRef'd at any nesting depth.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_decref_element(ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let rc_ptr = (ptr as usize - crate::rc::RC_HEADER_SIZE) as *mut usize;
        let rc = *rc_ptr;
        if (rc as isize) < 0 {
            return;
        }
        *rc_ptr -= 1;
        if *rc_ptr == 0 {
            miri_rt_array_free(ptr as *mut MiriArray);
        }
    }

    /// Sets the element drop function for an array.
    ///
    /// When set, `miri_rt_array_free` calls `fn_ptr` on each non-null element
    /// pointer so that managed elements (Lists, Maps, class instances) have
    /// their RC decremented when the array is dropped.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_set_elem_drop_fn(ptr: *mut MiriArray, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).elem_drop_fn = fn_ptr;
        }
    }

    /// Sets the `elem_clone_fn` callback for this array.
    ///
    /// When non-zero, `miri_rt_array_clone` calls this function on each element
    /// to obtain a deep copy instead of IncRef-ing the pointer.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_set_elem_clone_fn(ptr: *mut MiriArray, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).elem_clone_fn = fn_ptr;
        }
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
    /// If `elem_drop_fn` is set, calls it on the old element pointer before
    /// overwriting so that managed elements have their RC decremented.
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
        if arr.elem_drop_fn != 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(arr.elem_drop_fn);
            let slot = arr.data.add(index * arr.elem_size) as *const usize;
            let old_ptr = *slot;
            if old_ptr != 0 {
                drop_fn(old_ptr as *mut u8);
            }
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
    ///
    /// If `elem_drop_fn` is set, calls it on each old non-null element pointer
    /// before overwriting so that managed elements have their RC decremented.
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
        if arr.elem_drop_fn != 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(arr.elem_drop_fn);
            for i in 0..arr.elem_count {
                let slot = arr.data.add(i * arr.elem_size) as *const usize;
                let old_ptr = *slot;
                if old_ptr != 0 {
                    drop_fn(old_ptr as *mut u8);
                }
            }
        }
        for i in 0..arr.elem_count {
            let dest = arr.data.add(i * arr.elem_size);
            ptr::copy_nonoverlapping(elem, dest, arr.elem_size);
        }
    }

    /// Returns a clone of the array.
    ///
    /// If `elem_clone_fn` is set, calls it on each non-null element pointer to
    /// produce an independent deep copy (the clone owns fresh allocations).
    /// Otherwise, if `elem_drop_fn` is set, IncRefs every non-null element so
    /// both collections hold valid RC references — the existing shallow-clone path.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_clone(ptr: *const MiriArray) -> *mut MiriArray {
        if ptr.is_null() {
            return miri_rt_array_new(0, 0);
        }
        let src = &*ptr;
        let new_arr = miri_rt_array_new(src.elem_count, src.elem_size);
        if new_arr.is_null() {
            return new_arr;
        }
        if !src.data.is_null() && !(*new_arr).data.is_null() {
            ptr::copy_nonoverlapping(src.data, (*new_arr).data, src.byte_len());
        }
        (*new_arr).elem_drop_fn = src.elem_drop_fn;
        (*new_arr).elem_clone_fn = src.elem_clone_fn;
        if src.elem_clone_fn != 0 && !src.data.is_null() && src.elem_count > 0 && src.elem_size > 0
        {
            let clone_fn: unsafe extern "C" fn(*mut u8) -> *mut u8 =
                std::mem::transmute(src.elem_clone_fn);
            for i in 0..src.elem_count {
                let slot = (*new_arr).data.add(i * src.elem_size) as *mut usize;
                let ptr_val = *slot;
                if ptr_val != 0 {
                    let new_elem = clone_fn(ptr_val as *mut u8);
                    *slot = new_elem as usize;
                }
            }
        } else if src.elem_drop_fn != 0
            && !src.data.is_null()
            && src.elem_count > 0
            && src.elem_size > 0
        {
            for i in 0..src.elem_count {
                let slot = src.data.add(i * src.elem_size) as *const usize;
                let ptr_val = *slot;
                if ptr_val != 0 {
                    crate::rc::incref(ptr_val as *mut u8);
                }
            }
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

    /// Panics with a clear out-of-bounds error message.
    ///
    /// This provides a better debugging experience than crashing silently on
    /// a hardware trap.
    ///
    /// Uses `libc::_exit(1)` (not `std::process::abort()`) so the process
    /// terminates cleanly without raising SIGABRT — important on macOS, where
    /// SIGABRT spawns `ReportCrash` and serializes the test suite under load.
    /// `_exit` also skips atexit handlers, so the `MIRI_LEAK_CHECK` observer
    /// does not fire on intentional bounds-check exits.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_array_panic_oob(index: usize, len: usize) {
        use std::io::Write;
        eprintln!(
            "Runtime error: Array index out of bounds: the len is {} but the index is {}",
            len, index
        );
        let _ = std::io::stderr().flush();
        libc::_exit(1);
    }
} // pub mod ffi
