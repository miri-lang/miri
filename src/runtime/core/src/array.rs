//! Fixed-size array implementation for Miri runtime.
//!
//! Unlike `MiriList`, a `MiriArray` has a fixed element count determined at
//! creation time. The backing buffer is allocated once and never grows.
//! All elements are zeroed at creation.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr;

use crate::list::MiriList;

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

impl MiriArray {
    fn byte_len(&self) -> usize {
        self.elem_count * self.elem_size
    }
}

impl Drop for MiriArray {
    fn drop(&mut self) {
        if !self.data.is_null() && self.elem_count > 0 && self.elem_size > 0 {
            let layout = Layout::from_size_align(self.byte_len(), 8).unwrap();
            unsafe {
                dealloc(self.data, layout);
            }
        }
    }
}

// =============================================================================
// FFI Functions
// =============================================================================

/// Creates a new fixed-size array with the given element count and size.
///
/// All elements are zeroed. Returns null if the allocation fails or if
/// `elem_count` or `elem_size` is zero.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_array_new(
    elem_count: usize,
    elem_size: usize,
) -> *mut MiriArray {
    if elem_count == 0 || elem_size == 0 {
        return Box::into_raw(Box::new(MiriArray {
            data: ptr::null_mut(),
            elem_count: 0,
            elem_size,
        }));
    }

    let total = elem_count * elem_size;
    let layout = match Layout::from_size_align(total, 8) {
        Ok(layout) => layout,
        Err(_) => {
            return Box::into_raw(Box::new(MiriArray {
                data: ptr::null_mut(),
                elem_count: 0,
                elem_size,
            }));
        }
    };

    let data = alloc_zeroed(layout);
    if data.is_null() {
        return Box::into_raw(Box::new(MiriArray {
            data: ptr::null_mut(),
            elem_count: 0,
            elem_size,
        }));
    }

    Box::into_raw(Box::new(MiriArray {
        data,
        elem_count,
        elem_size,
    }))
}

/// Frees the array and its backing storage.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_array_free(ptr: *mut MiriArray) {
    if !ptr.is_null() {
        let _ = Box::from_raw(ptr);
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
pub unsafe extern "C" fn miri_rt_array_get(
    ptr: *const MiriArray,
    index: usize,
) -> *const u8 {
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
pub unsafe extern "C" fn miri_rt_array_get_mut(
    ptr: *mut MiriArray,
    index: usize,
) -> *mut u8 {
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
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(miri_rt_array_set(arr, 1, &val as *const i32 as *const u8), 1);

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
            assert_eq!(miri_rt_array_set(arr, 3, &val as *const i32 as *const u8), 0);

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
}
