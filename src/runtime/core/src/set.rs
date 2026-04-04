//! Generic hash set implementation for Miri runtime.
//!
//! Implements a type-erased hash set using open addressing with linear probing.
//! Elements are stored as opaque byte arrays. The Miri compiler provides
//! element size at each call site.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr;

use crate::rc::{alloc_with_rc, free_with_rc};

/// State flags for hash table slots.
const SLOT_EMPTY: u8 = 0;
const SLOT_OCCUPIED: u8 = 1;
const SLOT_TOMBSTONE: u8 = 2;

/// Initial capacity for new sets.
const INITIAL_CAPACITY: usize = 8;

/// Load factor threshold. Resize when len > capacity * 3/4.
const LOAD_FACTOR_NUM: usize = 3;
const LOAD_FACTOR_DEN: usize = 4;

/// A type-erased hash set using open addressing with linear probing.
///
/// Memory layout matches what Miri codegen expects:
/// - `data`: pointer to element storage (contiguous, elem_size per slot)
/// - `len`: number of occupied entries
/// - `states`: pointer to slot state array (EMPTY/OCCUPIED/TOMBSTONE)
/// - `capacity`: total number of slots
/// - `elem_size`: size of each element in bytes
///
/// The first two fields (`data`, `len`) match MiriList/MiriArray layout so
/// that `Rvalue::Len` and `element_at` use the same offsets.
#[repr(C)]
pub struct MiriSet {
    data: *mut u8,
    len: usize,
    states: *mut u8,
    capacity: usize,
    elem_size: usize,
}

const STRUCT_SIZE: usize = std::mem::size_of::<MiriSet>();

/// FNV-1a hash for raw byte sequences.
fn fnv1a(data: *const u8, len: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for i in 0..len {
        hash ^= unsafe { *data.add(i) } as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl MiriSet {
    /// Returns the slot index for a given element.
    unsafe fn find_slot(&self, elem: *const u8) -> Option<usize> {
        if self.capacity == 0 {
            return None;
        }
        let hash = fnv1a(elem, self.elem_size);
        let mut idx = (hash as usize) % self.capacity;
        for _ in 0..self.capacity {
            let state = *self.states.add(idx);
            if state == SLOT_EMPTY {
                return None;
            }
            if state == SLOT_OCCUPIED {
                let slot_data = self.data.add(idx * self.elem_size);
                if Self::bytes_equal(slot_data, elem, self.elem_size) {
                    return Some(idx);
                }
            }
            idx = (idx + 1) % self.capacity;
        }
        None
    }

    /// Finds a slot for insertion.
    ///
    /// Continues probing past tombstones to check for existing duplicates
    /// further in the probe chain. Returns the first available slot
    /// (tombstone or empty) only after confirming no duplicate exists.
    unsafe fn find_insert_slot(&self, elem: *const u8) -> usize {
        let hash = fnv1a(elem, self.elem_size);
        let mut idx = (hash as usize) % self.capacity;
        let mut first_tombstone: Option<usize> = None;
        for _ in 0..self.capacity {
            let state = *self.states.add(idx);
            match state {
                SLOT_EMPTY => {
                    return first_tombstone.unwrap_or(idx);
                }
                SLOT_OCCUPIED => {
                    let slot_data = self.data.add(idx * self.elem_size);
                    if Self::bytes_equal(slot_data, elem, self.elem_size) {
                        return idx; // duplicate found
                    }
                }
                SLOT_TOMBSTONE => {
                    if first_tombstone.is_none() {
                        first_tombstone = Some(idx);
                    }
                }
                _ => {}
            }
            idx = (idx + 1) % self.capacity;
        }
        // Table is full (shouldn't happen with proper load factor)
        first_tombstone.unwrap_or(0)
    }

    fn bytes_equal(a: *const u8, b: *const u8, len: usize) -> bool {
        for i in 0..len {
            if unsafe { *a.add(i) != *b.add(i) } {
                return false;
            }
        }
        true
    }

    unsafe fn ensure_capacity(&mut self) {
        if self.capacity == 0 {
            self.alloc_tables(INITIAL_CAPACITY);
            return;
        }
        if self.len * LOAD_FACTOR_DEN > self.capacity * LOAD_FACTOR_NUM {
            self.grow();
        }
    }

    unsafe fn alloc_tables(&mut self, capacity: usize) {
        let states_layout =
            Layout::from_size_align(capacity, 1).unwrap_or_else(|_| std::process::abort());
        let data_size = capacity
            .checked_mul(self.elem_size)
            .unwrap_or_else(|| std::process::abort());
        let data_layout =
            Layout::from_size_align(data_size, 8).unwrap_or_else(|_| std::process::abort());
        self.states = alloc_zeroed(states_layout);
        self.data = alloc_zeroed(data_layout);
        self.capacity = capacity;
    }

    unsafe fn grow(&mut self) {
        let old_states = self.states;
        let old_data = self.data;
        let old_capacity = self.capacity;

        let new_capacity = old_capacity.checked_mul(2).unwrap_or_else(|| std::process::abort());
        self.alloc_tables(new_capacity);
        self.len = 0;

        for i in 0..old_capacity {
            if *old_states.add(i) == SLOT_OCCUPIED {
                let elem = old_data.add(i * self.elem_size);
                let slot = self.find_insert_slot(elem);
                ptr::copy_nonoverlapping(
                    elem,
                    self.data.add(slot * self.elem_size),
                    self.elem_size,
                );
                *self.states.add(slot) = SLOT_OCCUPIED;
                self.len += 1;
            }
        }

        Self::free_tables(old_states, old_data, old_capacity, self.elem_size);
    }

    unsafe fn free_tables(states: *mut u8, data: *mut u8, capacity: usize, elem_size: usize) {
        if !states.is_null() && capacity > 0 {
            let states_layout =
                Layout::from_size_align(capacity, 1).unwrap_or_else(|_| std::process::abort());
            dealloc(states, states_layout);
        }
        if !data.is_null() && capacity > 0 && elem_size > 0 {
            let data_layout = Layout::from_size_align(
                capacity.checked_mul(elem_size).unwrap_or_else(|| std::process::abort()),
                8,
            )
            .unwrap_or_else(|_| std::process::abort());
            dealloc(data, data_layout);
        }
    }

    fn contains_key(&self, elem: *const u8) -> bool {
        unsafe { self.find_slot(elem).is_some() }
    }

    unsafe fn insert(&mut self, elem: *const u8) -> bool {
        self.ensure_capacity();
        let slot = self.find_insert_slot(elem);
        if *self.states.add(slot) == SLOT_OCCUPIED {
            return false; // duplicate
        }
        ptr::copy_nonoverlapping(elem, self.data.add(slot * self.elem_size), self.elem_size);
        *self.states.add(slot) = SLOT_OCCUPIED;
        self.len += 1;
        true
    }

    unsafe fn remove_elem(&mut self, elem: *const u8) -> bool {
        if let Some(idx) = self.find_slot(elem) {
            *self.states.add(idx) = SLOT_TOMBSTONE;
            self.len -= 1;
            true
        } else {
            false
        }
    }
}

// =============================================================================
// FFI Functions
// =============================================================================

/// Stable FFI interface for set operations.
pub mod ffi {
    use super::*;
    use std::ptr;

    /// Creates a new empty set with the given element size.
    ///
    /// Allocates `[RC=1][MiriSet fields]`.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_new(elem_size: usize) -> *mut MiriSet {
        let payload = alloc_with_rc(STRUCT_SIZE);
        if payload.is_null() {
            return ptr::null_mut();
        }
        let set = payload as *mut MiriSet;
        (*set).data = ptr::null_mut();
        (*set).len = 0;
        (*set).states = ptr::null_mut();
        (*set).capacity = 0;
        (*set).elem_size = elem_size;
        set
    }

    /// Returns the number of elements in the set.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_len(ptr: *const MiriSet) -> usize {
        if ptr.is_null() {
            return 0;
        }
        (*ptr).len
    }

    /// Returns true (1) if the set is empty, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_is_empty(ptr: *const MiriSet) -> u8 {
        if ptr.is_null() {
            return 1;
        }
        if (*ptr).len == 0 {
            1
        } else {
            0
        }
    }

    /// Adds an element to the set.
    ///
    /// The value is passed as a pointer-sized integer. The runtime copies
    /// `elem_size` bytes from the address of the parameter on the stack.
    /// Returns true (1) if the element was newly inserted, false (0) if duplicate.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_add(ptr: *mut MiriSet, elem: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let set = &mut *ptr;
        if set.insert(&elem as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Returns true (1) if the set contains the given element.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_contains(ptr: *const MiriSet, elem: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let set = &*ptr;
        if set.contains_key(&elem as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Removes an element from the set.
    /// Returns true (1) if removed, false (0) if not found.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_remove(ptr: *mut MiriSet, elem: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let set = &mut *ptr;
        if set.remove_elem(&elem as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Removes all elements from the set.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_clear(ptr: *mut MiriSet) {
        if ptr.is_null() {
            return;
        }
        let set = &mut *ptr;
        if !set.states.is_null() && set.capacity > 0 {
            ptr::write_bytes(set.states, 0, set.capacity);
        }
        set.len = 0;
    }

    /// Returns the element at the given sequential index (skipping empty/tombstone slots).
    ///
    /// This enables iteration via `element_at` in for-loops.
    /// Returns the element value as a usize.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_element_at(ptr: *const MiriSet, index: usize) -> usize {
        if ptr.is_null() {
            return 0;
        }
        let set = &*ptr;
        let mut count: usize = 0;
        for i in 0..set.capacity {
            if *set.states.add(i) == SLOT_OCCUPIED {
                if count == index {
                    let elem_ptr = set.data.add(i * set.elem_size);
                    return *(elem_ptr as *const usize);
                }
                count += 1;
            }
        }
        0
    }

    /// Frees a set and all its backing storage.
    ///
    /// The pointer must have been returned by `miri_rt_set_new` (points past RC header).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_set_free(ptr: *mut MiriSet) {
        if ptr.is_null() {
            return;
        }
        let set = &*ptr;
        MiriSet::free_tables(set.states, set.data, set.capacity, set.elem_size);
        free_with_rc(ptr as *mut u8, STRUCT_SIZE);
    }
} // pub mod ffi

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::ffi::*;

    #[test]
    fn test_set_new_empty() {
        unsafe {
            let set = miri_rt_set_new(8);
            assert!(!set.is_null());
            assert_eq!(miri_rt_set_len(set), 0);
            assert_eq!(miri_rt_set_is_empty(set), 1);
            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_add_contains() {
        unsafe {
            let set = miri_rt_set_new(8);

            assert_eq!(miri_rt_set_add(set, 10), 1);
            assert_eq!(miri_rt_set_add(set, 20), 1);
            assert_eq!(miri_rt_set_add(set, 10), 0); // duplicate

            assert_eq!(miri_rt_set_len(set), 2);
            assert_eq!(miri_rt_set_contains(set, 10), 1);
            assert_eq!(miri_rt_set_contains(set, 20), 1);
            assert_eq!(miri_rt_set_contains(set, 30), 0);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_remove() {
        unsafe {
            let set = miri_rt_set_new(8);

            miri_rt_set_add(set, 42);
            assert_eq!(miri_rt_set_len(set), 1);

            assert_eq!(miri_rt_set_remove(set, 42), 1);
            assert_eq!(miri_rt_set_len(set), 0);
            assert_eq!(miri_rt_set_remove(set, 42), 0); // not found

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_clear() {
        unsafe {
            let set = miri_rt_set_new(8);

            for i in 0..5usize {
                miri_rt_set_add(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 5);

            miri_rt_set_clear(set);
            assert_eq!(miri_rt_set_len(set), 0);
            assert_eq!(miri_rt_set_is_empty(set), 1);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_element_at() {
        unsafe {
            let set = miri_rt_set_new(8);

            miri_rt_set_add(set, 10);
            miri_rt_set_add(set, 20);
            miri_rt_set_add(set, 30);

            let mut elements = Vec::new();
            for i in 0..3 {
                elements.push(miri_rt_set_element_at(set, i));
            }
            elements.sort();
            assert_eq!(elements, vec![10, 20, 30]);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_grow() {
        unsafe {
            let set = miri_rt_set_new(8);

            for i in 0..20usize {
                miri_rt_set_add(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 20);

            for i in 0..20usize {
                assert_eq!(miri_rt_set_contains(set, i), 1);
            }

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_dedup_on_construction() {
        unsafe {
            let set = miri_rt_set_new(8);

            miri_rt_set_add(set, 1);
            miri_rt_set_add(set, 2);
            miri_rt_set_add(set, 2);
            miri_rt_set_add(set, 3);
            miri_rt_set_add(set, 3);
            miri_rt_set_add(set, 3);

            assert_eq!(miri_rt_set_len(set), 3);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_rc_header() {
        unsafe {
            let set = miri_rt_set_new(8);
            assert!(!set.is_null());

            let rc_ptr = (set as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *const usize;
            assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

            miri_rt_set_free(set);
        }
    }

    /// Regression test for tombstone probe-chain bug:
    /// After removing an element, re-adding a colliding element must not
    /// create a duplicate.
    #[test]
    fn test_set_remove_then_readd_no_duplicate() {
        unsafe {
            let set = miri_rt_set_new(8);

            // Insert values that may collide in the hash table
            for i in 0..6usize {
                miri_rt_set_add(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 6);

            // Remove some elements (creates tombstones)
            miri_rt_set_remove(set, 1);
            miri_rt_set_remove(set, 3);
            assert_eq!(miri_rt_set_len(set), 4);

            // Re-add a value that still exists — must be a no-op
            assert_eq!(miri_rt_set_add(set, 2), 0); // duplicate
            assert_eq!(miri_rt_set_len(set), 4);

            // Re-add removed values — should work
            assert_eq!(miri_rt_set_add(set, 1), 1);
            assert_eq!(miri_rt_set_add(set, 3), 1);
            assert_eq!(miri_rt_set_len(set), 6);

            // Adding them again must be a no-op
            assert_eq!(miri_rt_set_add(set, 1), 0);
            assert_eq!(miri_rt_set_add(set, 3), 0);
            assert_eq!(miri_rt_set_len(set), 6);

            // All values must be present
            for i in 0..6usize {
                assert_eq!(miri_rt_set_contains(set, i), 1, "missing element {i}");
            }

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_heavy_remove_readd_cycle() {
        unsafe {
            let set = miri_rt_set_new(8);

            // Insert 50 elements
            for i in 0..50usize {
                miri_rt_set_add(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 50);

            // Remove even numbers
            for i in (0..50usize).step_by(2) {
                miri_rt_set_remove(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 25);

            // Verify odd numbers still present, even numbers gone
            for i in 0..50usize {
                if i % 2 == 0 {
                    assert_eq!(miri_rt_set_contains(set, i), 0);
                } else {
                    assert_eq!(miri_rt_set_contains(set, i), 1);
                }
            }

            // Re-add even numbers
            for i in (0..50usize).step_by(2) {
                assert_eq!(miri_rt_set_add(set, i), 1);
            }
            assert_eq!(miri_rt_set_len(set), 50);

            // All should be present
            for i in 0..50usize {
                assert_eq!(miri_rt_set_contains(set, i), 1);
            }

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_null_safety() {
        unsafe {
            assert_eq!(miri_rt_set_len(std::ptr::null()), 0);
            assert_eq!(miri_rt_set_is_empty(std::ptr::null()), 1);
            assert_eq!(miri_rt_set_add(std::ptr::null_mut(), 42), 0);
            assert_eq!(miri_rt_set_contains(std::ptr::null(), 42), 0);
            assert_eq!(miri_rt_set_remove(std::ptr::null_mut(), 42), 0);
            assert_eq!(miri_rt_set_element_at(std::ptr::null(), 0), 0);
            miri_rt_set_clear(std::ptr::null_mut()); // must not crash
            miri_rt_set_free(std::ptr::null_mut()); // must not crash
        }
    }

    #[test]
    fn test_set_element_at_out_of_bounds() {
        unsafe {
            let set = miri_rt_set_new(8);
            miri_rt_set_add(set, 42);

            assert_eq!(miri_rt_set_element_at(set, 0), 42);
            assert_eq!(miri_rt_set_element_at(set, 1), 0); // out of bounds
            assert_eq!(miri_rt_set_element_at(set, 100), 0);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_clear_then_reuse() {
        unsafe {
            let set = miri_rt_set_new(8);

            for i in 0..10usize {
                miri_rt_set_add(set, i);
            }
            miri_rt_set_clear(set);

            // Should be able to add elements again
            for i in 100..110usize {
                miri_rt_set_add(set, i);
            }
            assert_eq!(miri_rt_set_len(set), 10);

            // Old elements gone, new ones present
            assert_eq!(miri_rt_set_contains(set, 0), 0);
            assert_eq!(miri_rt_set_contains(set, 100), 1);

            miri_rt_set_free(set);
        }
    }

    #[test]
    fn test_set_single_element() {
        unsafe {
            let set = miri_rt_set_new(8);

            miri_rt_set_add(set, 99);
            assert_eq!(miri_rt_set_len(set), 1);
            assert_eq!(miri_rt_set_is_empty(set), 0);
            assert_eq!(miri_rt_set_contains(set, 99), 1);
            assert_eq!(miri_rt_set_element_at(set, 0), 99);

            miri_rt_set_remove(set, 99);
            assert_eq!(miri_rt_set_len(set), 0);
            assert_eq!(miri_rt_set_is_empty(set), 1);

            miri_rt_set_free(set);
        }
    }
}
