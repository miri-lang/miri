// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
/// - `elem_drop_fn`: If non-zero, called on each element pointer when that element is
///   removed by a mutation operation (`clear`, `remove_at`). Allows managed elements
///   (Lists, Maps, class instances) to have their RC decremented on removal.
///   Set by `miri_rt_list_new_from_managed_array` when elements are heap-allocated.
/// - `elem_clone_fn`: If non-zero, called on each element pointer during
///   `miri_rt_list_clone` to produce a deep copy instead of an IncRef.
///   Signature: `fn(*mut u8) -> *mut u8`. Must only be set for user-defined class
///   elements that implement `Cloneable`.
#[repr(C)]
pub struct MiriList {
    data: *mut u8,
    len: usize,
    capacity: usize,
    elem_size: usize,
    /// Drop function for managed elements: `fn(elem_ptr: *mut u8)`.
    /// Zero means elements are plain values (no RC management on removal).
    elem_drop_fn: usize,
    /// Clone function for managed elements: `fn(*mut u8) -> *mut u8`.
    /// When non-zero, `miri_rt_list_clone` calls this instead of IncRef-ing.
    elem_clone_fn: usize,
}

impl MiriList {
    /// Creates a new empty list with the given element size.
    pub fn new(elem_size: usize) -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
            capacity: 0,
            elem_size,
            elem_drop_fn: 0,
            elem_clone_fn: 0,
        }
    }

    /// Creates a new list with pre-allocated capacity.
    pub fn with_capacity(elem_size: usize, capacity: usize) -> Self {
        if capacity == 0 || elem_size == 0 {
            return Self::new(elem_size);
        }

        let size = match capacity.checked_mul(elem_size) {
            Some(s) => s,
            None => return Self::new(elem_size),
        };
        let layout = match Layout::from_size_align(size, 8) {
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
            elem_drop_fn: 0,
            elem_clone_fn: 0,
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
            let old_size = self
                .capacity
                .checked_mul(self.elem_size)
                .unwrap_or_else(|| std::process::abort());
            match Layout::from_size_align(old_size, 8) {
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
    ///
    /// If `elem_drop_fn` is set, calls it on each element pointer before clearing
    /// so that managed elements (Lists, Maps, etc.) have their RC decremented.
    pub fn clear(&mut self) {
        if self.elem_drop_fn != 0 && !self.data.is_null() && self.len > 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) =
                unsafe { std::mem::transmute(self.elem_drop_fn) };
            for i in 0..self.len {
                unsafe {
                    let slot = self.data.add(i * self.elem_size) as *const usize;
                    let elem_ptr = *slot;
                    if elem_ptr != 0 {
                        drop_fn(elem_ptr as *mut u8);
                    }
                }
            }
        }
        self.len = 0;
    }
}

impl Drop for MiriList {
    fn drop(&mut self) {
        if !self.data.is_null() && self.capacity > 0 && self.elem_size > 0 {
            let size = self
                .capacity
                .checked_mul(self.elem_size)
                .unwrap_or_else(|| std::process::abort());
            if let Ok(layout) = Layout::from_size_align(size, 8) {
                unsafe {
                    dealloc(self.data, layout);
                }
            }
        }
    }
}

/// Stable FFI interface for list operations.
pub mod ffi {
    use super::read_as_i64;
    use super::*;
    use std::alloc::{alloc, dealloc, Layout};
    use std::ptr;

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

    /// Creates a new list from a MiriArray whose elements are RC-managed pointers.
    ///
    /// Same as `miri_rt_list_new_from_raw` but IncRefs each non-null element pointer
    /// after copying. This is necessary when elements are heap-allocated (Option,
    /// List, Array, Map, Set, Tuple, Custom) because the caller's array will
    /// release its element references via the element-drop loop when freed. Without
    /// this IncRef the list would hold dangling pointers.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_new_from_managed_array(
        array: *mut crate::array::MiriArray,
        _len: usize,
        _elem_size: usize,
    ) -> *mut MiriList {
        let list = miri_rt_list_new_from_raw(array, _len, _elem_size);
        if list.is_null() || array.is_null() {
            return list;
        }
        // IncRef each element in the newly-created list so the list owns a
        // reference independent of the source array.
        let list_ref = &*list;
        let data = list_ref.data;
        let len = list_ref.len;
        let elem_size = list_ref.elem_size;
        if data.is_null() || len == 0 || elem_size == 0 {
            return list;
        }
        for i in 0..len {
            let slot = data.add(i * elem_size) as *const usize;
            let ptr_val = *slot;
            if ptr_val != 0 {
                // RC is stored at ptr - RC_HEADER_SIZE (one word before the payload)
                let rc_ptr = (ptr_val as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *mut usize;
                let rc = *rc_ptr;
                // Skip immortal objects (RC high bit set — e.g. string literals)
                if (rc as isize) >= 0 {
                    *rc_ptr = rc + 1;
                }
            }
        }
        // Mark this list as holding managed (heap-allocated) elements. When elements
        // are later removed by mutation operations (clear, remove_at), elem_drop_fn
        // is called so that each removed element's RC is decremented.
        //
        // NOTE: This sets a single drop function for all managed element types. For
        // List-of-List cases the function handles one level of recursion. Deeper
        // nesting (List<List<List<T>>>) is handled correctly for the normal drop path
        // (variables going out of scope) by the codegen's element-drop loops, but
        // mutation operations on lists holding non-List managed elements would need
        // a different drop function (future work).
        (*list).elem_drop_fn = miri_rt_list_decref_element as *const () as usize;
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
        (*list).elem_drop_fn = 0;
        (*list).elem_clone_fn = 0;
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
        let size = match capacity.checked_mul(elem_size) {
            Some(s) => s,
            None => return list,
        };
        let layout = match Layout::from_size_align(size, 8) {
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
    ///
    /// If `elem_drop_fn` is set, calls it on the old element pointer before
    /// overwriting so that managed elements have their RC decremented.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_set(ptr: *mut MiriList, index: usize, val: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let list = &mut *ptr;
        if index >= list.len {
            return 0;
        }
        if list.elem_drop_fn != 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(list.elem_drop_fn);
            let slot = list.data.add(index * list.elem_size) as *const usize;
            let old_ptr = *slot;
            if old_ptr != 0 {
                drop_fn(old_ptr as *mut u8);
            }
        }
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
    pub unsafe extern "C" fn miri_rt_list_insert(
        ptr: *mut MiriList,
        index: usize,
        val: usize,
    ) -> u8 {
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
    ///
    /// If `elem_drop_fn` is set, calls it on the removed element pointer so that
    /// managed elements have their RC decremented on removal.
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

        // DecRef the element being removed if managed.
        if list.elem_drop_fn != 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(list.elem_drop_fn);
            let slot = list.data.add(index * list.elem_size) as *const usize;
            let elem_ptr = *slot;
            if elem_ptr != 0 {
                drop_fn(elem_ptr as *mut u8);
            }
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

    /// Sets the element drop function for a list.
    ///
    /// When set, mutation operations (`clear`, `remove_at`, `remove`) call `fn_ptr`
    /// on each removed element pointer so that managed elements (Lists, Maps, class
    /// instances) have their RC decremented on removal.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_set_elem_drop_fn(ptr: *mut MiriList, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).elem_drop_fn = fn_ptr;
        }
    }

    /// Sets the `elem_clone_fn` callback for this list.
    ///
    /// When non-zero, `miri_rt_list_clone` calls this function on each element
    /// to obtain a deep copy instead of IncRef-ing the pointer.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_set_elem_clone_fn(ptr: *mut MiriList, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).elem_clone_fn = fn_ptr;
        }
    }

    /// Decrements the RC of a managed List element and frees it if RC reaches zero.
    ///
    /// Used as `elem_drop_fn` by outer collections (Array, List, Set, Map) when
    /// they remove or overwrite a List-typed element at runtime (e.g. clear,
    /// remove, or element overwrite).  Unlike the Perceus scope-exit path — which
    /// emits an inline codegen loop to DecRef managed elements before calling
    /// `miri_rt_list_free` — this runtime callback has no such loop.  We therefore
    /// call `elem_drop_fn` on every live element here, before delegating to
    /// `miri_rt_list_free`, so that managed elements (e.g. List, Set, Map) nested
    /// inside the element list are correctly DecRef'd and never leaked.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_decref_element(ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let rc_ptr = (ptr as usize - crate::rc::RC_HEADER_SIZE) as *mut usize;
        let rc = *rc_ptr;
        // Skip immortal objects (RC stored as negative isize)
        if (rc as isize) < 0 {
            return;
        }
        *rc_ptr -= 1;
        if *rc_ptr == 0 {
            // DecRef managed elements before freeing.  The Perceus inline codegen
            // loop handles this for scope-exit drops; here we must do it ourselves.
            let list = ptr as *mut MiriList;
            if (*list).elem_drop_fn != 0 && (*list).elem_size > 0 && !(*list).data.is_null() {
                let drop_fn: unsafe extern "C" fn(*mut u8) =
                    std::mem::transmute((*list).elem_drop_fn);
                for i in 0..(*list).len {
                    let slot = (*list).data.add(i * (*list).elem_size) as *const usize;
                    let elem_ptr = *slot;
                    if elem_ptr != 0 {
                        drop_fn(elem_ptr as *mut u8);
                    }
                }
            }
            miri_rt_list_free(ptr as *mut MiriList);
        }
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
    ///
    /// If `elem_clone_fn` is set, calls it on each non-null element pointer to
    /// produce an independent deep copy (the clone owns fresh allocations).
    /// Otherwise, if `elem_drop_fn` is set, IncRefs every non-null element so
    /// both collections hold valid RC references — the existing shallow-clone path.
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

        (*list).elem_drop_fn = src.elem_drop_fn;
        (*list).elem_clone_fn = src.elem_clone_fn;

        if src.elem_clone_fn != 0 && !src.data.is_null() && src.len > 0 && src.elem_size > 0 {
            let clone_fn: unsafe extern "C" fn(*mut u8) -> *mut u8 =
                std::mem::transmute(src.elem_clone_fn);
            for i in 0..src.len {
                let slot = (*list).data.add(i * src.elem_size) as *mut usize;
                let ptr_val = *slot;
                if ptr_val != 0 {
                    let new_elem = clone_fn(ptr_val as *mut u8);
                    *slot = new_elem as usize;
                }
            }
        } else if src.elem_drop_fn != 0 && !src.data.is_null() && src.len > 0 && src.elem_size > 0 {
            for i in 0..src.len {
                let slot = src.data.add(i * src.elem_size) as *const usize;
                let ptr_val = *slot;
                if ptr_val != 0 {
                    crate::rc::incref(ptr_val as *mut u8);
                }
            }
        }

        list
    }

    /// Copy-on-Write check: if the list has more than one owner, produce an
    /// independent clone and decrement the old RC. Returns the (possibly new)
    /// pointer that the caller should now use.
    ///
    /// Invariant: the caller must treat the returned pointer as freshly owned
    /// (RC=1). The old pointer's RC is decremented inside this function and
    /// must not be used again by the caller.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_cow(ptr: *mut MiriList) -> *mut MiriList {
        if ptr.is_null() {
            return ptr;
        }
        let rc_ptr = (ptr as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *mut usize;
        let rc = *rc_ptr;
        // Negative RC means immortal — never copy.
        if (rc as isize) < 0 || rc <= 1 {
            return ptr;
        }
        let new_ptr = miri_rt_list_clone(ptr);
        if new_ptr.is_null() {
            return ptr;
        }
        *rc_ptr -= 1;
        new_ptr
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
            let size = list
                .capacity
                .checked_mul(list.elem_size)
                .unwrap_or_else(|| std::process::abort());
            let layout = Layout::from_size_align(size, 8).unwrap_or_else(|_| std::process::abort());
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
} // pub mod ffi

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
            std::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), copy_len);
            i64::from_ne_bytes(buf)
        }
    }
}
