// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Generic list (dynamic array) implementation for Miri runtime.
//!
//! Since Miri is a generic language but the runtime operates on raw bytes,
//! we implement a type-erased vector that stores elements as opaque byte arrays.
//! The Miri compiler provides element size information at each call site.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

use crate::element_order::ElementOrder;
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
/// - `elem_order`: How `miri_rt_list_sort` orders two elements: by reading their
///   bytes as a signed, unsigned or float value, or through a comparator
///   registered for element types whose bytes are a reference rather than a
///   value, which have no order of their own to read.
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
    /// How `sort` orders two elements; see [`ElementOrder`].
    elem_order: ElementOrder,
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
            elem_order: ElementOrder::SIGNED,
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
            elem_order: ElementOrder::SIGNED,
        }
    }

    /// Orders this list's elements the way `order` does, for a list copied
    /// from a container whose elements must sort alike.
    pub(crate) fn set_element_order(&mut self, order: ElementOrder) {
        self.elem_order = order;
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
                Ok(old_layout) => unsafe {
                    // `realloc` releases the old block when it moves, so the
                    // guard must witness that free here; growing a list is not a
                    // double free even though the same address may come back.
                    crate::guard::guard_free_raw(self.data);
                    realloc(self.data, old_layout, new_size)
                },
                Err(_) => std::process::abort(), // Abort safely rather than risking memory corruption
            }
        };

        if !new_data.is_null() {
            crate::guard::guard_alloc_raw(new_data, crate::guard::AllocKind::Buffer);
            crate::alloc_count::increment_buffer_count();
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
        if let Some(dest) = self.open_slot(self.len) {
            ptr::copy_nonoverlapping(elem, dest, self.elem_size);
        }
    }

    /// Appends an element stored inline: `payload` component bytes read from
    /// `src`, with the rest of the slot zeroed.
    ///
    /// # Safety
    /// - `src` must point to valid memory of at least `payload` bytes.
    /// - `payload` must not exceed `elem_size` (see [`Self::fits_inline_element`]).
    pub unsafe fn push_inline(&mut self, src: *const u8, payload: usize) {
        if let Some(dest) = self.open_slot(self.len) {
            write_inline_element(dest, src, payload, self.elem_size);
        }
    }

    /// Inserts an element stored inline at `index`, shifting later elements.
    /// Returns false when `index` is past the end.
    ///
    /// # Safety
    /// Same as [`Self::push_inline`].
    pub unsafe fn insert_inline(&mut self, index: usize, src: *const u8, payload: usize) -> bool {
        match self.open_slot(index) {
            Some(dest) => {
                write_inline_element(dest, src, payload, self.elem_size);
                true
            }
            None => false,
        }
    }

    /// Whether an element laid out at `stride` bytes, of which `payload` are
    /// real, has exactly the slot this list was allocated with.
    ///
    /// The compiler reads such an element back at `stride`, so a list whose
    /// slots are any other size would hand every later element back from the
    /// wrong offset.
    pub fn fits_inline_element(&self, payload: usize, stride: usize) -> bool {
        stride == self.elem_size && payload <= stride
    }

    /// Whether a whole element fits in the value word the word-passing entry
    /// points receive. A wider element would be copied from past the end of
    /// that word.
    ///
    /// TODO: an `i128`/`u128` list has 16-byte slots and so is refused here; its
    /// elements need to travel by address the way inline vectors do.
    pub fn fits_value_word(&self) -> bool {
        self.elem_size <= std::mem::size_of::<usize>()
    }

    /// Makes room for one element at `index`, shifting the elements from
    /// `index` on up by one, and returns the uninitialized slot. Returns `None`
    /// when `index` is past the end.
    unsafe fn open_slot(&mut self, index: usize) -> Option<*mut u8> {
        if index > self.len {
            return None;
        }

        self.reserve(1);

        if self.len >= self.capacity {
            return None;
        }

        if index < self.len {
            let src = self.data.add(index * self.elem_size);
            let dest = self.data.add((index + 1) * self.elem_size);
            let count = (self.len - index) * self.elem_size;
            ptr::copy(src, dest, count);
        }

        self.len += 1;
        Some(self.data.add(index * self.elem_size))
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
        match self.open_slot(index) {
            Some(dest) => {
                ptr::copy_nonoverlapping(elem, dest, self.elem_size);
                true
            }
            None => false,
        }
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
                    crate::guard::guard_free_raw(self.data);
                    dealloc(self.data, layout);
                }
            }
        }
    }
}

/// Writes an inline element into `dest`: the slot is zeroed first, so the
/// padding past the `payload` real bytes never carries stale memory, then the
/// payload is copied in.
///
/// # Safety
/// - `dest` must be valid for `elem_size` bytes and `src` for `payload` bytes.
/// - `payload` must not exceed `elem_size`.
unsafe fn write_inline_element(dest: *mut u8, src: *const u8, payload: usize, elem_size: usize) {
    ptr::write_bytes(dest, 0, elem_size);
    ptr::copy_nonoverlapping(src, dest, payload);
}

/// Stable FFI interface for list operations.
pub mod ffi {
    use super::*;
    use crate::guard;
    use std::alloc::{alloc, dealloc, Layout};
    use std::ptr;

    /// Creates a new list from a MiriArray.
    /// This is used by the compiler to lower `List([1, 2, 3])` constructor calls.
    /// The array's data and element order are copied into the new list; the
    /// array is NOT consumed.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_new_from_raw(
        array: *mut crate::array::MiriArray,
        _len: usize,
        elem_size: usize,
    ) -> *mut MiriList {
        let arr_elem_size = if !array.is_null() {
            (*array).elem_size()
        } else {
            8
        };
        let target_elem_size = if elem_size > 0 {
            elem_size
        } else {
            arr_elem_size
        };
        let list = miri_rt_list_new(target_elem_size);
        if array.is_null() || list.is_null() {
            return list;
        }
        let arr = &*array;
        (*list).set_element_order(arr.element_order());
        let data = arr.data_ptr();
        let len = arr.len();
        if data.is_null() {
            return list;
        }
        for i in 0..len {
            (*list).push(data.add(i * arr_elem_size));
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
        // mutation operations on lists holding non-List managed elements require
        // a different drop function to avoid incorrect cleanup.
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
        (*list).elem_order = ElementOrder::SIGNED;
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
            guard::guard_alloc_raw(data, guard::AllocKind::Buffer);
            crate::alloc_count::increment_buffer_count();
            (*list).data = data;
            (*list).capacity = capacity;
        }
        list
    }

    /// Returns the number of elements in the list.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_len(ptr: *const MiriList) -> usize {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        (*ptr).len()
    }

    /// Returns the capacity of the list.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_capacity(ptr: *const MiriList) -> usize {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        (*ptr).capacity()
    }

    /// Returns true (1) if the list is empty, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_is_empty(ptr: *const MiriList) -> u8 {
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return;
        }
        let list = &mut *ptr;
        if !list.fits_value_word() {
            std::process::abort();
        }
        list.push(&val as *const usize as *const u8);
    }

    /// Appends an element the list stores inline, handed by address.
    ///
    /// `src` points at the element's `payload` component bytes, and `stride` is
    /// the spacing the compiler addresses the list's elements at. A list whose
    /// slots are not exactly `stride` bytes cannot hold the element where an
    /// index read will look for it, so the process is aborted rather than the
    /// element stored.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_push_inline(
        ptr: *mut MiriList,
        src: *const u8,
        payload: usize,
        stride: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() || src.is_null() {
            return;
        }
        let list = &mut *ptr;
        if !list.fits_inline_element(payload, stride) {
            std::process::abort();
        }
        list.push_inline(src, payload);
    }

    /// Pops the last element from the list.
    /// Returns true (1) if successful, false (0) if the list was empty.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_pop(ptr: *mut MiriList) -> u8 {
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return ptr::null();
        }
        (*ptr).get(index)
    }

    /// Gets a mutable pointer to the element at the given index.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_get_mut(ptr: *mut MiriList, index: usize) -> *mut u8 {
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        let list = &mut *ptr;
        if !list.fits_value_word() {
            std::process::abort();
        }
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        let list = &mut *ptr;
        if !list.fits_value_word() {
            std::process::abort();
        }
        if list.insert(index, &val as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Inserts an element the list stores inline, handed by address.
    /// Returns true (1) if successful, false (0) if the index was out of bounds.
    ///
    /// The arguments and the refusal follow `miri_rt_list_push_inline`.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_insert_inline(
        ptr: *mut MiriList,
        index: usize,
        src: *const u8,
        payload: usize,
        stride: usize,
    ) -> u8 {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() || src.is_null() {
            return 0;
        }
        let list = &mut *ptr;
        if !list.fits_inline_element(payload, stride) {
            std::process::abort();
        }
        u8::from(list.insert_inline(index, src, payload))
    }

    /// Removes the element at the given index and releases it.
    /// Returns true (1) if successful, false (0) if the index was out of bounds.
    ///
    /// If `elem_drop_fn` is set, calls it on the removed element pointer so that
    /// managed elements have their RC decremented on removal. Use
    /// `miri_rt_list_take_at` where the element is handed to a caller instead.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_remove(ptr: *mut MiriList, index: usize) -> u8 {
        remove_at_index(ptr, index, true)
    }

    /// Removes the element at the given index without releasing it, handing the
    /// list's own reference to the caller.
    /// Returns true (1) if successful, false (0) if the index was out of bounds.
    ///
    /// A caller that reads the element out and then removes it holds a pointer
    /// the list is about to drop: releasing it here would free the value while
    /// that read is still live. Transferring the reference instead leaves the
    /// element with exactly one owner, the caller, which releases it as usual.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_take_at(ptr: *mut MiriList, index: usize) -> u8 {
        remove_at_index(ptr, index, false)
    }

    /// Drop the element at `index` out of the list, releasing it only when the
    /// list still owns it after the removal.
    unsafe fn remove_at_index(ptr: *mut MiriList, index: usize, release_element: bool) -> u8 {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        let list = &mut *ptr;
        if index >= list.len {
            return 0;
        }

        if release_element && list.elem_drop_fn != 0 {
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
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
        if !ptr.is_null() {
            (*ptr).elem_clone_fn = fn_ptr;
        }
    }

    /// Registers the comparator `miri_rt_list_sort` orders elements through.
    ///
    /// When non-zero, `fn_ptr` is an
    /// [`crate::element_order::ElementCompareFn`] called with the values two
    /// slots hold, instead of reading those slots as numbers.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_set_elem_compare_fn(ptr: *mut MiriList, fn_ptr: usize) {
        guard::guard_check(ptr as *mut u8);
        if !ptr.is_null() {
            (*ptr).elem_order.set_compare_fn(fn_ptr);
        }
    }

    /// Selects how `miri_rt_list_sort` reads an element's bytes as its value: one of
    /// the kinds in [`crate::element_order`]. Consulted only while no
    /// comparator is registered.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_set_elem_order_kind(ptr: *mut MiriList, kind: usize) {
        guard::guard_check(ptr as *mut u8);
        if !ptr.is_null() {
            (*ptr).elem_order.set_kind(kind);
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
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
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
        (*list).elem_order = src.elem_order;

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
        guard::guard_check(ptr as *mut u8);
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
            guard::guard_free_raw(list.data);
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() || (*ptr).is_empty() {
            return ptr::null();
        }
        (*ptr).get(0)
    }

    /// Returns the last element pointer, or null if empty.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_last(ptr: *const MiriList) -> *const u8 {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() || (*ptr).is_empty() {
            return ptr::null();
        }
        (*ptr).get((*ptr).len() - 1)
    }

    /// Sorts the list in ascending order.
    ///
    /// Elements are ordered by the comparator registered for the element type,
    /// or, when none is, by their bytes read as the signed, unsigned or float
    /// value the registered order kind names.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_sort(ptr: *mut MiriList) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return;
        }
        let list = &mut *ptr;
        crate::element_order::sort_elements(list.data, list.len, list.elem_size, list.elem_order);
    }

    /// Reverses the list in place.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_list_reverse(ptr: *mut MiriList) {
        guard::guard_check(ptr as *mut u8);
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
