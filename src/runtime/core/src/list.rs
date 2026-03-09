//! Generic list (dynamic array) implementation for Miri runtime.
//!
//! Since Miri is a generic language but the runtime operates on raw bytes,
//! we implement a type-erased vector that stores elements as opaque byte arrays.
//! The Miri compiler provides element size information at each call site.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

use crate::rc::{alloc_with_rc, free_with_rc};

/// A type-erased dynamic array.
///
/// Stores elements as contiguous bytes. The element size is provided
/// by the caller for each operation.
///
/// Memory layout matches what Miri expects:
/// - `data`: Pointer to element storage
/// - `len`: Number of elements (not bytes)
/// - `capacity`: Allocated capacity in elements
/// - `elem_size`: Size of each element in bytes
#[repr(C)]
pub struct MiriList {
    data: *mut u8,
    len: usize,
    capacity: usize,
    elem_size: usize,
}

impl MiriList {
    /// Creates a new empty list with the given element size.
    pub fn new(elem_size: usize) -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
            capacity: 0,
            elem_size,
        }
    }

    /// Creates a new list with pre-allocated capacity.
    pub fn with_capacity(elem_size: usize, capacity: usize) -> Self {
        if capacity == 0 || elem_size == 0 {
            return Self::new(elem_size);
        }

        let layout = match Layout::from_size_align(capacity * elem_size, 8) {
            Ok(layout) => layout,
            Err(_) => return Self::new(elem_size),
        };

        let data = unsafe { alloc(layout) };
        if data.is_null() {
            return Self::new(elem_size);
        }

        Self {
            data,
            len: 0,
            capacity,
            elem_size,
        }
    }

    /// Returns the number of elements.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the capacity in elements.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Ensures capacity for at least `additional` more elements.
    fn reserve(&mut self, additional: usize) {
        let required = self.len.saturating_add(additional);
        if required <= self.capacity {
            return;
        }

        // Growth strategy: double or use required, whichever is larger
        let new_capacity = std::cmp::max(self.capacity.saturating_mul(2), required);
        let new_capacity = std::cmp::max(new_capacity, 4); // Minimum capacity

        let new_size = new_capacity
            .checked_mul(self.elem_size)
            .unwrap_or_else(|| std::process::abort());
        let layout = match Layout::from_size_align(new_size, 8) {
            Ok(layout) => layout,
            Err(_) => std::process::abort(),
        };

        let new_data = if self.data.is_null() {
            unsafe { alloc(layout) }
        } else {
            match Layout::from_size_align(self.capacity * self.elem_size, 8) {
                Ok(old_layout) => unsafe { realloc(self.data, old_layout, new_size) },
                Err(_) => std::process::abort(), // Abort safely rather than risking memory corruption
            }
        };

        if !new_data.is_null() {
            self.data = new_data;
            self.capacity = new_capacity;
        } else {
            std::process::abort(); // OOM should also abort safely
        }
    }

    /// Pushes an element (as raw bytes) to the end of the list.
    ///
    /// # Safety
    /// - `elem` must point to valid memory of at least `elem_size` bytes.
    pub unsafe fn push(&mut self, elem: *const u8) {
        self.reserve(1);

        if self.len < self.capacity {
            let dest = self.data.add(self.len * self.elem_size);
            ptr::copy_nonoverlapping(elem, dest, self.elem_size);
            self.len += 1;
        }
    }

    /// Pops the last element and copies it to `out`.
    ///
    /// Returns true if an element was popped, false if the list was empty.
    ///
    /// # Safety
    /// - `out` must point to valid memory of at least `elem_size` bytes.
    pub unsafe fn pop(&mut self, out: *mut u8) -> bool {
        if self.len == 0 {
            return false;
        }

        self.len -= 1;
        let src = self.data.add(self.len * self.elem_size);
        ptr::copy_nonoverlapping(src, out, self.elem_size);
        true
    }

    /// Gets a pointer to the element at the given index.
    ///
    /// Returns null if the index is out of bounds.
    pub fn get(&self, index: usize) -> *const u8 {
        if index >= self.len {
            return ptr::null();
        }
        unsafe { self.data.add(index * self.elem_size) }
    }

    /// Gets a mutable pointer to the element at the given index.
    ///
    /// Returns null if the index is out of bounds.
    pub fn get_mut(&mut self, index: usize) -> *mut u8 {
        if index >= self.len {
            return ptr::null_mut();
        }
        unsafe { self.data.add(index * self.elem_size) }
    }

    /// Sets the element at the given index.
    ///
    /// # Safety
    /// - `elem` must point to valid memory of at least `elem_size` bytes.
    /// - `index` must be less than `len`.
    pub unsafe fn set(&mut self, index: usize, elem: *const u8) -> bool {
        if index >= self.len {
            return false;
        }
        let dest = self.data.add(index * self.elem_size);
        ptr::copy_nonoverlapping(elem, dest, self.elem_size);
        true
    }

    /// Inserts an element at the given index, shifting subsequent elements.
    ///
    /// # Safety
    /// - `elem` must point to valid memory of at least `elem_size` bytes.
    pub unsafe fn insert(&mut self, index: usize, elem: *const u8) -> bool {
        if index > self.len {
            return false;
        }

        self.reserve(1);

        if self.len >= self.capacity {
            return false;
        }

        // Shift elements to make room
        if index < self.len {
            let src = self.data.add(index * self.elem_size);
            let dest = self.data.add((index + 1) * self.elem_size);
            let count = (self.len - index) * self.elem_size;
            ptr::copy(src, dest, count);
        }

        // Insert the new element
        let dest = self.data.add(index * self.elem_size);
        ptr::copy_nonoverlapping(elem, dest, self.elem_size);
        self.len += 1;
        true
    }

    /// Removes the element at the given index, shifting subsequent elements.
    ///
    /// # Safety
    /// - `out` must point to valid memory of at least `elem_size` bytes.
    pub unsafe fn remove(&mut self, index: usize, out: *mut u8) -> bool {
        if index >= self.len {
            return false;
        }

        // Copy the element to output
        let src = self.data.add(index * self.elem_size);
        ptr::copy_nonoverlapping(src, out, self.elem_size);

        // Shift elements down
        if index < self.len - 1 {
            let dest = self.data.add(index * self.elem_size);
            let src = self.data.add((index + 1) * self.elem_size);
            let count = (self.len - index - 1) * self.elem_size;
            ptr::copy(src, dest, count);
        }

        self.len -= 1;
        true
    }

    /// Clears all elements from the list.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl Drop for MiriList {
    fn drop(&mut self) {
        if !self.data.is_null() && self.capacity > 0 && self.elem_size > 0 {
            if let Ok(layout) = Layout::from_size_align(self.capacity * self.elem_size, 8) {
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

/// Creates a new list from a MiriArray.
/// This is used by the compiler to lower `List([1, 2, 3])` constructor calls.
/// The array's data is copied into the new list; the array is NOT consumed.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_new_from_raw(
    array: *mut crate::array::MiriArray,
    _len: usize,
    _elem_size: usize,
) -> *mut MiriList {
    if array.is_null() {
        // Fallback: use _elem_size if provided, otherwise default to 8
        let es = if _elem_size > 0 { _elem_size } else { 8 };
        return miri_rt_list_new(es);
    }
    let arr = &*array;
    let data = arr.data_ptr();
    let len = arr.len();
    let elem_size = arr.elem_size();
    if data.is_null() || len == 0 {
        return miri_rt_list_new(elem_size);
    }
    let list = miri_rt_list_new(elem_size);
    for i in 0..len {
        (*list).push(data.add(i * elem_size));
    }
    list
}

/// Creates a new empty list with the given element size.
///
/// Allocates `[RC=1][MiriList fields]`.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_new(elem_size: usize) -> *mut MiriList {
    let struct_size = std::mem::size_of::<MiriList>();
    let payload = alloc_with_rc(struct_size);
    if payload.is_null() {
        return ptr::null_mut();
    }
    let list = payload as *mut MiriList;
    (*list).data = ptr::null_mut();
    (*list).len = 0;
    (*list).capacity = 0;
    (*list).elem_size = elem_size;
    list
}

/// Creates a new list with pre-allocated capacity.
///
/// Allocates `[RC=1][MiriList fields]` with a pre-allocated data buffer.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_with_capacity(
    elem_size: usize,
    capacity: usize,
) -> *mut MiriList {
    let list = miri_rt_list_new(elem_size);
    if list.is_null() || capacity == 0 || elem_size == 0 {
        return list;
    }
    let layout = match Layout::from_size_align(capacity * elem_size, 8) {
        Ok(l) => l,
        Err(_) => return list,
    };
    let data = alloc(layout);
    if !data.is_null() {
        (*list).data = data;
        (*list).capacity = capacity;
    }
    list
}

/// Returns the number of elements in the list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_len(ptr: *const MiriList) -> usize {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).len()
}

/// Returns the capacity of the list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_capacity(ptr: *const MiriList) -> usize {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).capacity()
}

/// Returns true (1) if the list is empty, false (0) otherwise.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_is_empty(ptr: *const MiriList) -> u8 {
    if ptr.is_null() {
        return 1;
    }
    if (*ptr).is_empty() {
        1
    } else {
        0
    }
}

/// Pushes an element to the end of the list.
///
/// The value is passed as a pointer-sized integer. The runtime copies
/// `elem_size` bytes from the address of the parameter on the stack.
/// This works for all primitive element types (int, float, bool, pointers)
/// which fit in a single register.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_push(ptr: *mut MiriList, val: usize) {
    if ptr.is_null() {
        return;
    }
    let list = &mut *ptr;
    list.push(&val as *const usize as *const u8);
}

/// Pops the last element from the list.
/// Returns true (1) if successful, false (0) if the list was empty.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_pop(ptr: *mut MiriList) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if list.len == 0 {
        return 0;
    }
    list.len -= 1;
    1
}

/// Gets a pointer to the element at the given index.
/// Returns null if the index is out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_get(ptr: *const MiriList, index: usize) -> *const u8 {
    if ptr.is_null() {
        return ptr::null();
    }
    (*ptr).get(index)
}

/// Gets a mutable pointer to the element at the given index.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_get_mut(ptr: *mut MiriList, index: usize) -> *mut u8 {
    if ptr.is_null() {
        return ptr::null_mut();
    }
    (*ptr).get_mut(index)
}

/// Sets the element at the given index.
/// Returns true (1) if successful, false (0) if the index was out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_set(ptr: *mut MiriList, index: usize, val: usize) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if list.set(index, &val as *const usize as *const u8) {
        1
    } else {
        0
    }
}

/// Inserts an element at the given index.
/// Returns true (1) if successful, false (0) if the index was out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_insert(ptr: *mut MiriList, index: usize, val: usize) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if list.insert(index, &val as *const usize as *const u8) {
        1
    } else {
        0
    }
}

/// Removes the element at the given index.
/// Returns true (1) if successful, false (0) if the index was out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_remove(ptr: *mut MiriList, index: usize) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if index >= list.len {
        return 0;
    }

    // Shift elements down
    if index < list.len - 1 {
        let dest = list.data.add(index * list.elem_size);
        let src = list.data.add((index + 1) * list.elem_size);
        let count = (list.len - index - 1) * list.elem_size;
        ptr::copy(src, dest, count);
    }

    list.len -= 1;
    1
}

/// Clears all elements from the list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_clear(ptr: *mut MiriList) {
    if !ptr.is_null() {
        (*ptr).clear();
    }
}

/// Clones a list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_clone(ptr: *const MiriList) -> *mut MiriList {
    if ptr.is_null() {
        return miri_rt_list_new(0);
    }

    let src = &*ptr;
    let list = miri_rt_list_with_capacity(src.elem_size, src.len);
    if list.is_null() {
        return list;
    }

    if !src.data.is_null() && src.len > 0 && !(*list).data.is_null() {
        ptr::copy_nonoverlapping(src.data, (*list).data, src.len * src.elem_size);
        (*list).len = src.len;
    }

    list
}

/// Frees a list and its backing storage.
///
/// The pointer must have been returned by `miri_rt_list_new` (i.e., it
/// points past the RC header).
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_free(ptr: *mut MiriList) {
    if ptr.is_null() {
        return;
    }
    // Free internal data buffer
    let list = &*ptr;
    if !list.data.is_null() && list.capacity > 0 && list.elem_size > 0 {
        let layout = Layout::from_size_align(list.capacity * list.elem_size, 8)
            .unwrap_or_else(|_| std::process::abort());
        dealloc(list.data, layout);
    }
    // Free the [RC][struct] block
    let struct_size = std::mem::size_of::<MiriList>();
    free_with_rc(ptr as *mut u8, struct_size);
}

/// Returns the first element pointer, or null if empty.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_first(ptr: *const MiriList) -> *const u8 {
    if ptr.is_null() || (*ptr).is_empty() {
        return ptr::null();
    }
    (*ptr).get(0)
}

/// Returns the last element pointer, or null if empty.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_last(ptr: *const MiriList) -> *const u8 {
    if ptr.is_null() || (*ptr).is_empty() {
        return ptr::null();
    }
    (*ptr).get((*ptr).len() - 1)
}

/// Sorts the list in ascending order (elements compared as signed 64-bit integers).
///
/// Uses insertion sort which is stable and efficient for small lists.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_sort(ptr: *mut MiriList) {
    if ptr.is_null() {
        return;
    }
    let list = &mut *ptr;
    if list.len < 2 || list.data.is_null() {
        return;
    }

    let elem_size = list.elem_size;
    let mut temp = vec![0u8; elem_size];

    for i in 1..list.len {
        // Copy element[i] to temp
        let src = list.data.add(i * elem_size);
        ptr::copy_nonoverlapping(src, temp.as_mut_ptr(), elem_size);
        let key = read_as_i64(temp.as_ptr(), elem_size);

        let mut j = i;
        while j > 0 {
            let prev = list.data.add((j - 1) * elem_size);
            let prev_val = read_as_i64(prev, elem_size);
            if prev_val <= key {
                break;
            }
            // Shift element[j-1] to element[j]
            let dest = list.data.add(j * elem_size);
            ptr::copy_nonoverlapping(prev, dest, elem_size);
            j -= 1;
        }
        // Place temp at position j
        let dest = list.data.add(j * elem_size);
        ptr::copy_nonoverlapping(temp.as_ptr(), dest, elem_size);
    }
}

/// Reverses the list in place.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_reverse(ptr: *mut MiriList) {
    if ptr.is_null() {
        return;
    }

    let list = &mut *ptr;
    if list.len < 2 {
        return;
    }

    let elem_size = list.elem_size;
    let mut temp = vec![0u8; elem_size];

    let mut i = 0;
    let mut j = list.len - 1;

    while i < j {
        let left = list.data.add(i * elem_size);
        let right = list.data.add(j * elem_size);

        // Swap using temp buffer
        ptr::copy_nonoverlapping(left, temp.as_mut_ptr(), elem_size);
        ptr::copy_nonoverlapping(right, left, elem_size);
        ptr::copy_nonoverlapping(temp.as_ptr(), right, elem_size);

        i += 1;
        j -= 1;
    }
}

/// Reads raw bytes as a signed 64-bit integer for comparison purposes.
///
/// Handles common element sizes (1, 2, 4, 8 bytes) with sign extension.
/// Other sizes are zero-padded.
pub(crate) unsafe fn read_as_i64(ptr: *const u8, elem_size: usize) -> i64 {
    match elem_size {
        1 => *(ptr as *const i8) as i64,
        2 => *(ptr as *const i16) as i64,
        4 => *(ptr as *const i32) as i64,
        8 => *(ptr as *const i64),
        _ => {
            let mut buf = [0u8; 8];
            let copy_len = elem_size.min(8);
            ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), copy_len);
            i64::from_ne_bytes(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a list and push i32 values via the internal API.
    unsafe fn make_i32_list(values: &[i32]) -> *mut MiriList {
        let list = miri_rt_list_new(std::mem::size_of::<i32>());
        for val in values {
            (*list).push(val as *const i32 as *const u8);
        }
        list
    }

    #[test]
    fn test_list_push_pop() {
        unsafe {
            let list = make_i32_list(&[10, 20, 30]);
            assert_eq!(miri_rt_list_len(list), 3);

            let mut out: i32 = 0;
            assert!((*list).pop(&mut out as *mut i32 as *mut u8));
            assert_eq!(out, 30);
            assert_eq!(miri_rt_list_len(list), 2);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_get_set() {
        unsafe {
            let list = make_i32_list(&[100, 200, 300]);

            let ptr = miri_rt_list_get(list, 1);
            assert!(!ptr.is_null());
            assert_eq!(*(ptr as *const i32), 200);

            let new_val = 999i32;
            assert!((*list).set(1, &new_val as *const i32 as *const u8));

            let ptr = miri_rt_list_get(list, 1);
            assert_eq!(*(ptr as *const i32), 999);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_insert_remove() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3]);

            let insert_val = 99i32;
            assert!((*list).insert(1, &insert_val as *const i32 as *const u8));
            assert_eq!(miri_rt_list_len(list), 4);

            // Verify order: [1, 99, 2, 3]
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 99);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 2);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 3);

            // Remove at index 1
            let mut removed: i32 = 0;
            assert!((*list).remove(1, &mut removed as *mut i32 as *mut u8));
            assert_eq!(removed, 99);
            assert_eq!(miri_rt_list_len(list), 3);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_clone() {
        unsafe {
            let list = make_i32_list(&[5, 10, 15]);
            let cloned = miri_rt_list_clone(list);

            assert_eq!(miri_rt_list_len(cloned), 3);
            assert_eq!(*(miri_rt_list_get(cloned, 0) as *const i32), 5);
            assert_eq!(*(miri_rt_list_get(cloned, 1) as *const i32), 10);
            assert_eq!(*(miri_rt_list_get(cloned, 2) as *const i32), 15);

            miri_rt_list_free(list);
            miri_rt_list_free(cloned);
        }
    }

    #[test]
    fn test_list_reverse() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3, 4, 5]);
            miri_rt_list_reverse(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 5);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 4);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 3);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 2);
            assert_eq!(*(miri_rt_list_get(list, 4) as *const i32), 1);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 30usize);
            miri_rt_list_push(list, 10usize);
            miri_rt_list_push(list, 20usize);
            miri_rt_list_push(list, 5usize);
            miri_rt_list_sort(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 5);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 10);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 20);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const usize), 30);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort_already_sorted() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 1usize);
            miri_rt_list_push(list, 2usize);
            miri_rt_list_push(list, 3usize);
            miri_rt_list_sort(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 2);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 3);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort_single_element() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 42usize);
            miri_rt_list_sort(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 42);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_ffi_list_push() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 42);
            miri_rt_list_push(list, 100);

            assert_eq!(miri_rt_list_len(list), 2);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 42);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 100);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_null_safety() {
        unsafe {
            assert_eq!(miri_rt_list_len(std::ptr::null()), 0);
            assert_eq!(miri_rt_list_capacity(std::ptr::null()), 0);
            assert_eq!(miri_rt_list_is_empty(std::ptr::null()), 1);
            miri_rt_list_push(std::ptr::null_mut(), 42); // must not crash
            assert_eq!(miri_rt_list_pop(std::ptr::null_mut()), 0);
            assert!(miri_rt_list_get(std::ptr::null(), 0).is_null());
            assert!(miri_rt_list_get_mut(std::ptr::null_mut(), 0).is_null());
            assert_eq!(miri_rt_list_set(std::ptr::null_mut(), 0, 42), 0);
            assert_eq!(miri_rt_list_insert(std::ptr::null_mut(), 0, 42), 0);
            assert_eq!(miri_rt_list_remove(std::ptr::null_mut(), 0), 0);
            miri_rt_list_clear(std::ptr::null_mut()); // must not crash
            assert!(miri_rt_list_first(std::ptr::null()).is_null());
            assert!(miri_rt_list_last(std::ptr::null()).is_null());
            miri_rt_list_sort(std::ptr::null_mut()); // must not crash
            miri_rt_list_reverse(std::ptr::null_mut()); // must not crash
            miri_rt_list_free(std::ptr::null_mut()); // must not crash
        }
    }

    #[test]
    fn test_list_empty_operations() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            assert_eq!(miri_rt_list_is_empty(list), 1);
            assert_eq!(miri_rt_list_pop(list), 0);
            assert!(miri_rt_list_first(list).is_null());
            assert!(miri_rt_list_last(list).is_null());
            assert!(miri_rt_list_get(list, 0).is_null());
            assert_eq!(miri_rt_list_set(list, 0, 42), 0);
            assert_eq!(miri_rt_list_remove(list, 0), 0);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_insert_at_beginning() {
        unsafe {
            let list = make_i32_list(&[2, 3, 4]);

            let val = 1i32;
            assert!((*list).insert(0, &val as *const i32 as *const u8));
            assert_eq!(miri_rt_list_len(list), 4);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 2);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_insert_at_end() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3]);

            let val = 4i32;
            assert!((*list).insert(3, &val as *const i32 as *const u8));
            assert_eq!(miri_rt_list_len(list), 4);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 4);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_insert_out_of_bounds() {
        unsafe {
            let list = make_i32_list(&[1, 2]);

            let val = 99i32;
            assert!(!(*list).insert(5, &val as *const i32 as *const u8));
            assert_eq!(miri_rt_list_len(list), 2); // unchanged

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_remove_first() {
        unsafe {
            let list = make_i32_list(&[10, 20, 30]);

            let mut removed: i32 = 0;
            assert!((*list).remove(0, &mut removed as *mut i32 as *mut u8));
            assert_eq!(removed, 10);
            assert_eq!(miri_rt_list_len(list), 2);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 20);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_remove_last() {
        unsafe {
            let list = make_i32_list(&[10, 20, 30]);

            let mut removed: i32 = 0;
            assert!((*list).remove(2, &mut removed as *mut i32 as *mut u8));
            assert_eq!(removed, 30);
            assert_eq!(miri_rt_list_len(list), 2);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_remove_out_of_bounds() {
        unsafe {
            let list = make_i32_list(&[1, 2]);

            let mut removed: i32 = 0;
            assert!(!(*list).remove(5, &mut removed as *mut i32 as *mut u8));
            assert_eq!(miri_rt_list_len(list), 2); // unchanged

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_first_last() {
        unsafe {
            let list = make_i32_list(&[10, 20, 30]);

            assert_eq!(*(miri_rt_list_first(list) as *const i32), 10);
            assert_eq!(*(miri_rt_list_last(list) as *const i32), 30);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_first_last_single() {
        unsafe {
            let list = make_i32_list(&[42]);

            assert_eq!(*(miri_rt_list_first(list) as *const i32), 42);
            assert_eq!(*(miri_rt_list_last(list) as *const i32), 42);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort_negative_values() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            // Use i64 values cast through usize FFI interface
            miri_rt_list_push(list, (-5i64) as usize);
            miri_rt_list_push(list, 3usize);
            miri_rt_list_push(list, (-1i64) as usize);
            miri_rt_list_push(list, 0usize);

            miri_rt_list_sort(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const i64), -5);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i64), -1);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i64), 0);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i64), 3);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort_duplicates() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 3usize);
            miri_rt_list_push(list, 1usize);
            miri_rt_list_push(list, 3usize);
            miri_rt_list_push(list, 2usize);
            miri_rt_list_push(list, 1usize);
            miri_rt_list_sort(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 1);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 2);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const usize), 3);
            assert_eq!(*(miri_rt_list_get(list, 4) as *const usize), 3);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_sort_empty() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_sort(list); // must not crash
            assert_eq!(miri_rt_list_len(list), 0);
            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_reverse_even_count() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3, 4]);
            miri_rt_list_reverse(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 4);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 3);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 2);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 1);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_reverse_single() {
        unsafe {
            let list = make_i32_list(&[42]);
            miri_rt_list_reverse(list); // must not crash
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 42);
            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_reverse_empty() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());
            miri_rt_list_reverse(list); // must not crash
            assert_eq!(miri_rt_list_len(list), 0);
            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_clone_empty() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());
            let cloned = miri_rt_list_clone(list);
            assert_eq!(miri_rt_list_len(cloned), 0);
            miri_rt_list_free(list);
            miri_rt_list_free(cloned);
        }
    }

    #[test]
    fn test_list_clone_null() {
        unsafe {
            let cloned = miri_rt_list_clone(std::ptr::null());
            assert!(!cloned.is_null());
            assert_eq!(miri_rt_list_len(cloned), 0);
            miri_rt_list_free(cloned);
        }
    }

    #[test]
    fn test_list_clone_independence() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3]);
            let cloned = miri_rt_list_clone(list);

            // Modify original
            let val = 99i32;
            (*list).set(0, &val as *const i32 as *const u8);

            // Clone unaffected
            assert_eq!(*(miri_rt_list_get(cloned, 0) as *const i32), 1);

            miri_rt_list_free(list);
            miri_rt_list_free(cloned);
        }
    }

    #[test]
    fn test_list_with_capacity() {
        unsafe {
            let list = miri_rt_list_with_capacity(std::mem::size_of::<usize>(), 10);
            assert!(!list.is_null());
            assert_eq!(miri_rt_list_len(list), 0);
            assert!(miri_rt_list_capacity(list) >= 10);

            // Push should work without reallocation
            for i in 0..10usize {
                miri_rt_list_push(list, i);
            }
            assert_eq!(miri_rt_list_len(list), 10);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_clear() {
        unsafe {
            let list = make_i32_list(&[1, 2, 3]);
            miri_rt_list_clear(list);
            assert_eq!(miri_rt_list_len(list), 0);
            assert_eq!(miri_rt_list_is_empty(list), 1);

            // Can push again after clear
            let val = 99i32;
            (*list).push(&val as *const i32 as *const u8);
            assert_eq!(miri_rt_list_len(list), 1);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 99);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_growth_stress() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());

            // Push many elements to trigger multiple reallocations
            for i in 0..1000usize {
                miri_rt_list_push(list, i);
            }
            assert_eq!(miri_rt_list_len(list), 1000);

            // Verify all values
            for i in 0..1000usize {
                assert_eq!(*(miri_rt_list_get(list, i) as *const usize), i);
            }

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_ffi_remove() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 10usize);
            miri_rt_list_push(list, 20usize);
            miri_rt_list_push(list, 30usize);

            assert_eq!(miri_rt_list_remove(list, 1), 1);
            assert_eq!(miri_rt_list_len(list), 2);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 10);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 30);

            // Out of bounds
            assert_eq!(miri_rt_list_remove(list, 5), 0);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_ffi_insert() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 1usize);
            miri_rt_list_push(list, 3usize);

            assert_eq!(miri_rt_list_insert(list, 1, 2usize), 1);
            assert_eq!(miri_rt_list_len(list), 3);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 2);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 3);

            // Out of bounds
            assert_eq!(miri_rt_list_insert(list, 10, 99usize), 0);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_ffi_set() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<usize>());
            miri_rt_list_push(list, 1usize);
            miri_rt_list_push(list, 2usize);

            assert_eq!(miri_rt_list_set(list, 0, 99usize), 1);
            assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 99);

            // Out of bounds
            assert_eq!(miri_rt_list_set(list, 5, 99usize), 0);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_rc_header() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());
            assert!(!list.is_null());

            let rc_ptr = (list as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *const usize;
            assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

            miri_rt_list_free(list);
        }
    }
}
