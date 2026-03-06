//! Generic list (dynamic array) implementation for Miri runtime.
//!
//! Since Miri is a generic language but the runtime operates on raw bytes,
//! we implement a type-erased vector that stores elements as opaque byte arrays.
//! The Miri compiler provides element size information at each call site.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

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

        let new_size = new_capacity * self.elem_size;
        let layout = match Layout::from_size_align(new_size, 8) {
            Ok(layout) => layout,
            Err(_) => std::process::abort(), // Abort safely rather than risking memory corruption
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

/// Creates a new list from a raw memory buffer containing elements.
/// This is used by the compiler to lower List([1, 2, 3]) literals.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_new_from_raw(
    data: *const u8,
    len: usize,
    elem_size: usize,
) -> *mut MiriList {
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
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_new(elem_size: usize) -> *mut MiriList {
    let list = Box::new(MiriList::new(elem_size));
    Box::into_raw(list)
}

/// Creates a new list with pre-allocated capacity.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_with_capacity(
    elem_size: usize,
    capacity: usize,
) -> *mut MiriList {
    let list = Box::new(MiriList::with_capacity(elem_size, capacity));
    Box::into_raw(list)
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
    if (*ptr).is_empty() { 1 } else { 0 }
}

/// Pushes an element to the end of the list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_push(ptr: *mut MiriList, val: u64) {
    if ptr.is_null() {
        return;
    }
    let list = &mut *ptr;
    list.push(&val as *const u64 as *const u8);
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
pub unsafe extern "C" fn miri_rt_list_set(
    ptr: *mut MiriList,
    index: usize,
    val: u64,
) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if list.set(index, &val as *const u64 as *const u8) { 1 } else { 0 }
}

/// Inserts an element at the given index.
/// Returns true (1) if successful, false (0) if the index was out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_insert(
    ptr: *mut MiriList,
    index: usize,
    val: u64,
) -> u8 {
    if ptr.is_null() {
        return 0;
    }
    let list = &mut *ptr;
    if list.insert(index, &val as *const u64 as *const u8) { 1 } else { 0 }
}

/// Removes the element at the given index.
/// Returns true (1) if successful, false (0) if the index was out of bounds.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_remove(
    ptr: *mut MiriList,
    index: usize,
) -> u8 {
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
    let mut new_list = MiriList::with_capacity(src.elem_size, src.len);

    if !src.data.is_null() && src.len > 0 {
        ptr::copy_nonoverlapping(src.data, new_list.data, src.len * src.elem_size);
        new_list.len = src.len;
    }

    Box::into_raw(Box::new(new_list))
}

/// Frees a list.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_list_free(ptr: *mut MiriList) {
    if !ptr.is_null() {
        let _ = Box::from_raw(ptr);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_push_pop() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            let values = [10i32, 20, 30];
            for val in &values {
                miri_rt_list_push(list, *val as u64);
            }

            assert_eq!(miri_rt_list_len(list), 3);

            assert_eq!(miri_rt_list_pop(list), 1);

            assert_eq!(miri_rt_list_len(list), 2);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_get_set() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            let values = [100i32, 200, 300];
            for val in &values {
                miri_rt_list_push(list, *val as u64);
            }

            // Get element
            let ptr = miri_rt_list_get(list, 1);
            assert!(!ptr.is_null());
            assert_eq!(*(ptr as *const i32), 200);

            // Set element
            let new_val = 999i32;
            assert_eq!(miri_rt_list_set(list, 1, new_val as u64), 1);

            let ptr = miri_rt_list_get(list, 1);
            assert_eq!(*(ptr as *const i32), 999);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_insert_remove() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            let values = [1i32, 2, 3];
            for val in &values {
                miri_rt_list_push(list, *val as u64);
            }

            // Insert at index 1
            let insert_val = 99i32;
            assert_eq!(miri_rt_list_insert(list, 1, insert_val as u64), 1);
            assert_eq!(miri_rt_list_len(list), 4);

            // Verify order: [1, 99, 2, 3]
            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 1);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 99);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 2);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 3);

            // Remove at index 1
            assert_eq!(miri_rt_list_remove(list, 1), 1);
            assert_eq!(miri_rt_list_len(list), 3);

            miri_rt_list_free(list);
        }
    }

    #[test]
    fn test_list_clone() {
        unsafe {
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            let values = [5i32, 10, 15];
            for val in &values {
                miri_rt_list_push(list, *val as u64);
            }

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
            let list = miri_rt_list_new(std::mem::size_of::<i32>());

            let values = [1i32, 2, 3, 4, 5];
            for val in &values {
                miri_rt_list_push(list, *val as u64);
            }

            miri_rt_list_reverse(list);

            assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 5);
            assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 4);
            assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 3);
            assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 2);
            assert_eq!(*(miri_rt_list_get(list, 4) as *const i32), 1);

            miri_rt_list_free(list);
        }
    }
}
