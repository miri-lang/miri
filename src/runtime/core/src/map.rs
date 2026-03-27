//! Generic hash map implementation for Miri runtime.
//!
//! Implements a type-erased hash map using open addressing with linear probing.
//! Keys and values are stored as opaque byte arrays. The Miri compiler provides
//! key/value sizes and a key kind tag at each call site.
//!
//! Key kinds:
//! - 0: value type (int, float, bool) — hashed/compared by raw bytes
//! - 1: string type — dereferences MiriString pointer for hash/compare

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr;

use crate::rc::{alloc_with_rc, free_with_rc};
use crate::string::MiriString;

/// State flags for hash table slots.
const SLOT_EMPTY: u8 = 0;
const SLOT_OCCUPIED: u8 = 1;
const SLOT_TOMBSTONE: u8 = 2;

/// Initial capacity for new maps.
const INITIAL_CAPACITY: usize = 8;

/// Load factor threshold (numerator/denominator). Resize when len > capacity * 3/4.
const LOAD_FACTOR_NUM: usize = 3;
const LOAD_FACTOR_DEN: usize = 4;

/// A type-erased hash map using open addressing with linear probing.
///
/// Memory layout matches what Miri codegen expects:
/// - `states`: pointer to slot state array (EMPTY/OCCUPIED/TOMBSTONE)
/// - `keys`: pointer to key storage (contiguous, key_size per slot)
/// - `values`: pointer to value storage (contiguous, value_size per slot)
/// - `len`: number of occupied entries
/// - `capacity`: total number of slots
/// - `key_size`: size of each key in bytes
/// - `value_size`: size of each value in bytes
/// - `key_kind`: 0 = value type, 1 = string type
/// - `val_drop_fn`: If non-zero, called on each value pointer when that entry is
///   removed by a mutation operation (`remove`, `clear`, or `set` overwriting an
///   existing key). Allows managed values (Lists, Maps, etc.) to have their RC
///   decremented on removal. Set via `miri_rt_map_set_val_drop_fn`.
#[repr(C)]
pub struct MiriMap {
    states: *mut u8,
    keys: *mut u8,
    values: *mut u8,
    len: usize,
    capacity: usize,
    key_size: usize,
    value_size: usize,
    key_kind: usize,
    /// Drop function for managed values: `fn(val_ptr: *mut u8)`.
    /// Zero means values are plain (no RC management on removal).
    val_drop_fn: usize,
}

const STRUCT_SIZE: usize = std::mem::size_of::<MiriMap>();

/// FNV-1a hash for raw byte sequences.
fn fnv1a(data: *const u8, len: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for i in 0..len {
        hash ^= unsafe { *data.add(i) } as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Compares two byte sequences for equality.
unsafe fn bytes_equal(a: *const u8, b: *const u8, len: usize) -> bool {
    for i in 0..len {
        if *a.add(i) != *b.add(i) {
            return false;
        }
    }
    true
}

impl MiriMap {
    /// Computes the hash of a key.
    unsafe fn hash_key(&self, key: *const u8) -> u64 {
        if self.key_kind == 1 {
            // String key: the key bytes contain a pointer to MiriString
            let str_ptr = *(key as *const *const MiriString);
            if str_ptr.is_null() {
                return 0;
            }
            let s = &*str_ptr;
            if s.data.is_null() || s.len == 0 {
                return fnv1a(ptr::null(), 0);
            }
            fnv1a(s.data, s.len)
        } else {
            // Value key: hash the raw bytes
            fnv1a(key, self.key_size)
        }
    }

    /// Compares two keys for equality.
    unsafe fn keys_equal(&self, a: *const u8, b: *const u8) -> bool {
        if self.key_kind == 1 {
            // String key: compare string contents
            let ptr_a = *(a as *const *const MiriString);
            let ptr_b = *(b as *const *const MiriString);
            if ptr_a.is_null() && ptr_b.is_null() {
                return true;
            }
            if ptr_a.is_null() || ptr_b.is_null() {
                return false;
            }
            let sa = &*ptr_a;
            let sb = &*ptr_b;
            if sa.len != sb.len {
                return false;
            }
            if sa.len == 0 {
                return true;
            }
            bytes_equal(sa.data, sb.data, sa.len)
        } else {
            // Value key: compare raw bytes
            bytes_equal(a, b, self.key_size)
        }
    }

    /// Finds the slot for a given key (for lookup or insertion).
    /// Returns (slot_index, found) where found indicates if the key was found.
    unsafe fn find_slot(&self, key: *const u8) -> (usize, bool) {
        if self.capacity == 0 {
            return (0, false);
        }
        let hash = self.hash_key(key);
        let mut idx = (hash as usize) % self.capacity;
        let mut first_tombstone: Option<usize> = None;

        for _ in 0..self.capacity {
            let state = *self.states.add(idx);
            match state {
                SLOT_EMPTY => {
                    // Key not found; return first tombstone if any, else this empty slot
                    return (first_tombstone.unwrap_or(idx), false);
                }
                SLOT_OCCUPIED => {
                    let existing_key = self.keys.add(idx * self.key_size);
                    if self.keys_equal(existing_key, key) {
                        return (idx, true);
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
        (first_tombstone.unwrap_or(0), false)
    }

    /// Allocates the internal arrays for a given capacity.
    unsafe fn alloc_tables(
        capacity: usize,
        key_size: usize,
        value_size: usize,
    ) -> Option<(*mut u8, *mut u8, *mut u8)> {
        if capacity == 0 || key_size == 0 {
            return None;
        }
        let states_layout = Layout::from_size_align(capacity, 1).ok()?;
        let keys_layout = Layout::from_size_align(capacity * key_size, 8).ok()?;
        // value_size can be 0 for value-less maps (unlikely but safe)
        let val_total = capacity * value_size.max(1);
        let values_layout = Layout::from_size_align(val_total, 8).ok()?;

        let states = alloc_zeroed(states_layout);
        if states.is_null() {
            return None;
        }
        let keys = alloc_zeroed(keys_layout);
        if keys.is_null() {
            dealloc(states, states_layout);
            return None;
        }
        let values = alloc_zeroed(values_layout);
        if values.is_null() {
            dealloc(states, states_layout);
            dealloc(keys, keys_layout);
            return None;
        }

        Some((states, keys, values))
    }

    /// Frees the internal arrays.
    unsafe fn free_tables(
        states: *mut u8,
        keys: *mut u8,
        values: *mut u8,
        capacity: usize,
        key_size: usize,
        value_size: usize,
    ) {
        if capacity == 0 {
            return;
        }
        if !states.is_null() {
            if let Ok(layout) = Layout::from_size_align(capacity, 1) {
                dealloc(states, layout);
            }
        }
        if !keys.is_null() {
            if let Some(key_total) = capacity.checked_mul(key_size) {
                if let Ok(layout) = Layout::from_size_align(key_total, 8) {
                    dealloc(keys, layout);
                }
            }
        }
        if !values.is_null() {
            if let Some(val_total) = capacity.checked_mul(value_size.max(1)) {
                if let Ok(layout) = Layout::from_size_align(val_total, 8) {
                    dealloc(values, layout);
                }
            }
        }
    }

    /// Grows the table and rehashes all entries.
    unsafe fn grow(&mut self) {
        let new_capacity = if self.capacity == 0 {
            INITIAL_CAPACITY
        } else {
            self.capacity * 2
        };

        let Some((new_states, new_keys, new_values)) =
            Self::alloc_tables(new_capacity, self.key_size, self.value_size)
        else {
            // OOM during resize — abort to prevent data corruption
            std::process::abort();
        };

        // Rehash existing entries
        let old_states = self.states;
        let old_keys = self.keys;
        let old_values = self.values;
        let old_capacity = self.capacity;

        self.states = new_states;
        self.keys = new_keys;
        self.values = new_values;
        self.capacity = new_capacity;
        self.len = 0;

        for i in 0..old_capacity {
            if *old_states.add(i) == SLOT_OCCUPIED {
                let key = old_keys.add(i * self.key_size);
                let value = old_values.add(i * self.value_size);
                self.insert_raw(key, value);
            }
        }

        Self::free_tables(
            old_states,
            old_keys,
            old_values,
            old_capacity,
            self.key_size,
            self.value_size,
        );
    }

    /// Inserts a key-value pair without checking load factor.
    unsafe fn insert_raw(&mut self, key: *const u8, value: *const u8) {
        let (idx, found) = self.find_slot(key);

        // If the key already exists and values are managed, DecRef the old value
        // before overwriting so the old object's RC is decremented correctly.
        if found && self.val_drop_fn != 0 && self.value_size > 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(self.val_drop_fn);
            let old_val_addr = self.values.add(idx * self.value_size) as *const usize;
            let old_val_ptr = *old_val_addr;
            if old_val_ptr != 0 {
                drop_fn(old_val_ptr as *mut u8);
            }
        }

        let dest_key = self.keys.add(idx * self.key_size);
        let dest_value = self.values.add(idx * self.value_size);

        ptr::copy_nonoverlapping(key, dest_key, self.key_size);
        if self.value_size > 0 {
            ptr::copy_nonoverlapping(value, dest_value, self.value_size);
        }
        if !found {
            *self.states.add(idx) = SLOT_OCCUPIED;
            self.len += 1;
        }
    }

    /// Sets a key-value pair, growing the table if necessary.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn set(&mut self, key: *const u8, value: *const u8) {
        // Check if we need to grow (before insertion to ensure capacity)
        let need_grow = self.capacity == 0
            || (self.len + 1) * LOAD_FACTOR_DEN > self.capacity * LOAD_FACTOR_NUM;
        if need_grow {
            self.grow();
        }
        self.insert_raw(key, value);
    }

    /// Gets a pointer to the value for a key, or null if not found.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn get(&self, key: *const u8) -> *const u8 {
        if self.capacity == 0 || self.len == 0 {
            return ptr::null();
        }
        let (idx, found) = self.find_slot(key);
        if found {
            self.values.add(idx * self.value_size)
        } else {
            ptr::null()
        }
    }

    /// Removes a key-value pair. Returns true if the key was found and removed.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn remove(&mut self, key: *const u8) -> bool {
        if self.capacity == 0 || self.len == 0 {
            return false;
        }
        let (idx, found) = self.find_slot(key);
        if found {
            // DecRef the value being removed if managed.
            if self.val_drop_fn != 0 && self.value_size > 0 {
                let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(self.val_drop_fn);
                let val_addr = self.values.add(idx * self.value_size) as *const usize;
                let val_ptr = *val_addr;
                if val_ptr != 0 {
                    drop_fn(val_ptr as *mut u8);
                }
            }
            *self.states.add(idx) = SLOT_TOMBSTONE;
            self.len -= 1;
            true
        } else {
            false
        }
    }

    /// Returns true if the map contains the given key.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn contains_key(&self, key: *const u8) -> bool {
        if self.capacity == 0 || self.len == 0 {
            return false;
        }
        let (_, found) = self.find_slot(key);
        found
    }

    /// Clears all entries from the map.
    pub fn clear(&mut self) {
        if self.capacity > 0 && !self.states.is_null() {
            unsafe {
                // DecRef all managed values before zeroing states.
                if self.val_drop_fn != 0 && self.value_size > 0 {
                    let drop_fn: unsafe extern "C" fn(*mut u8) =
                        std::mem::transmute(self.val_drop_fn);
                    for i in 0..self.capacity {
                        if *self.states.add(i) == SLOT_OCCUPIED {
                            let val_addr = self.values.add(i * self.value_size) as *const usize;
                            let val_ptr = *val_addr;
                            if val_ptr != 0 {
                                drop_fn(val_ptr as *mut u8);
                            }
                        }
                    }
                }
                ptr::write_bytes(self.states, 0, self.capacity);
            }
        }
        self.len = 0;
    }
}

impl Drop for MiriMap {
    fn drop(&mut self) {
        unsafe {
            Self::free_tables(
                self.states,
                self.keys,
                self.values,
                self.capacity,
                self.key_size,
                self.value_size,
            );
        }
    }
}

// =============================================================================
// FFI Functions
// =============================================================================

/// Stable FFI interface for map operations.
pub mod ffi {
    use super::*;
    use std::ptr;

    /// Creates a new empty map with the given key/value sizes and key kind.
    ///
    /// `key_kind`: 0 = value type (int/float/bool), 1 = string type.
    ///
    /// Allocates `[RC=1][MiriMap fields]`.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_new(
        key_size: usize,
        value_size: usize,
        key_kind: usize,
    ) -> *mut MiriMap {
        let payload = alloc_with_rc(STRUCT_SIZE);
        if payload.is_null() {
            return ptr::null_mut();
        }
        let map = payload as *mut MiriMap;
        (*map).states = ptr::null_mut();
        (*map).keys = ptr::null_mut();
        (*map).values = ptr::null_mut();
        (*map).len = 0;
        (*map).capacity = 0;
        (*map).key_size = key_size;
        (*map).value_size = value_size;
        (*map).key_kind = key_kind;
        (*map).val_drop_fn = 0;
        map
    }

    /// Returns the number of entries in the map.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_len(ptr: *const MiriMap) -> usize {
        if ptr.is_null() {
            return 0;
        }
        (*ptr).len
    }

    /// Returns true (1) if the map is empty, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_is_empty(ptr: *const MiriMap) -> u8 {
        if ptr.is_null() {
            return 1;
        }
        if (*ptr).len == 0 {
            1
        } else {
            0
        }
    }

    /// Sets a key-value pair in the map.
    ///
    /// Both key and value are passed as pointer-sized integers. The runtime copies
    /// `key_size`/`value_size` bytes from the address of each parameter on the stack.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set(ptr: *mut MiriMap, key: usize, value: usize) {
        if ptr.is_null() {
            return;
        }
        let map = &mut *ptr;
        map.set(
            &key as *const usize as *const u8,
            &value as *const usize as *const u8,
        );
    }

    /// Gets the value for a key, returning the value as a pointer-sized integer.
    ///
    /// Returns 0 if the key is not found.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_get(ptr: *const MiriMap, key: usize) -> usize {
        if ptr.is_null() {
            return 0;
        }
        let map = &*ptr;
        let result = map.get(&key as *const usize as *const u8);
        if result.is_null() {
            return 0;
        }
        // Read the stored value as usize (matches how values are stored via set)
        *(result as *const usize)
    }

    /// Gets the value for a key, aborting if the key is not found.
    ///
    /// Used for direct map indexing (`m[key]`). For safe access, use `m.get(key)`
    /// which returns an Option.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_get_checked(ptr: *const MiriMap, key: usize) -> usize {
        if ptr.is_null() {
            eprintln!("Runtime error: map index on null map");
            std::process::abort();
        }
        let map = &*ptr;
        let result = map.get(&key as *const usize as *const u8);
        if result.is_null() {
            eprintln!("Runtime error: map key not found");
            std::process::abort();
        }
        *(result as *const usize)
    }

    /// Returns true (1) if the map contains the given key.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_contains_key(ptr: *const MiriMap, key: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let map = &*ptr;
        if map.contains_key(&key as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Removes the entry with the given key.
    ///
    /// Returns true (1) if the key was found and removed, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_remove(ptr: *mut MiriMap, key: usize) -> u8 {
        if ptr.is_null() {
            return 0;
        }
        let map = &mut *ptr;
        if map.remove(&key as *const usize as *const u8) {
            1
        } else {
            0
        }
    }

    /// Clears all entries from the map.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_clear(ptr: *mut MiriMap) {
        if !ptr.is_null() {
            (*ptr).clear();
        }
    }

    /// Sets the drop function for managed values.
    ///
    /// When non-zero, this function is called with the value pointer whenever an
    /// entry is removed by `remove`, `clear`, or `set` overwriting an existing key.
    /// Must be called after `miri_rt_map_new` when values are heap-allocated (e.g. List).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_val_drop_fn(ptr: *mut MiriMap, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).val_drop_fn = fn_ptr;
        }
    }

    /// Returns the key at the nth occupied slot (0-based sequential index).
    ///
    /// This enables iteration over map keys via `element_at`.
    /// Returns 0 if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_key_at(ptr: *const MiriMap, nth: usize) -> usize {
        if ptr.is_null() {
            return 0;
        }
        let map = &*ptr;
        let mut count = 0usize;
        for i in 0..map.capacity {
            if *map.states.add(i) == SLOT_OCCUPIED {
                if count == nth {
                    let key_ptr = map.keys.add(i * map.key_size);
                    return *(key_ptr as *const usize);
                }
                count += 1;
            }
        }
        0
    }

    /// Returns the value at the nth occupied slot (0-based sequential index).
    ///
    /// This enables `for k, v in map` iteration.
    /// Returns 0 if the index is out of bounds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_value_at(ptr: *const MiriMap, nth: usize) -> usize {
        if ptr.is_null() {
            return 0;
        }
        let map = &*ptr;
        let mut count = 0usize;
        for i in 0..map.capacity {
            if *map.states.add(i) == SLOT_OCCUPIED {
                if count == nth {
                    let val_ptr = map.values.add(i * map.value_size);
                    return *(val_ptr as *const usize);
                }
                count += 1;
            }
        }
        0
    }

    /// Frees a map and all its backing storage.
    ///
    /// The pointer must have been returned by `miri_rt_map_new` (points past RC header).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_free(ptr: *mut MiriMap) {
        if ptr.is_null() {
            return;
        }
        // Free internal tables
        let map = &*ptr;
        MiriMap::free_tables(
            map.states,
            map.keys,
            map.values,
            map.capacity,
            map.key_size,
            map.value_size,
        );
        // Free the [RC][struct] block
        free_with_rc(ptr as *mut u8, STRUCT_SIZE);
    }
} // pub mod ffi

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::ffi::*;
    use super::*;

    #[test]
    fn test_map_new_empty() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);
            assert!(!map.is_null());
            assert_eq!(miri_rt_map_len(map), 0);
            assert_eq!(miri_rt_map_is_empty(map), 1);
            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_set_get_int_keys() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            miri_rt_map_set(map, 2, 200);
            miri_rt_map_set(map, 3, 300);

            assert_eq!(miri_rt_map_len(map), 3);
            assert_eq!(miri_rt_map_get(map, 1), 100);
            assert_eq!(miri_rt_map_get(map, 2), 200);
            assert_eq!(miri_rt_map_get(map, 3), 300);
            assert_eq!(miri_rt_map_get(map, 4), 0); // not found

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_overwrite() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            assert_eq!(miri_rt_map_get(map, 1), 100);

            miri_rt_map_set(map, 1, 999);
            assert_eq!(miri_rt_map_get(map, 1), 999);
            assert_eq!(miri_rt_map_len(map), 1); // length unchanged

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_contains_key() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 42, 1);
            assert_eq!(miri_rt_map_contains_key(map, 42), 1);
            assert_eq!(miri_rt_map_contains_key(map, 99), 0);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_remove() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            miri_rt_map_set(map, 2, 200);
            assert_eq!(miri_rt_map_len(map), 2);

            assert_eq!(miri_rt_map_remove(map, 1), 1);
            assert_eq!(miri_rt_map_len(map), 1);
            assert_eq!(miri_rt_map_get(map, 1), 0); // removed
            assert_eq!(miri_rt_map_get(map, 2), 200); // still there

            assert_eq!(miri_rt_map_remove(map, 99), 0); // not found

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_clear() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            miri_rt_map_set(map, 2, 200);
            assert_eq!(miri_rt_map_len(map), 2);

            miri_rt_map_clear(map);
            assert_eq!(miri_rt_map_len(map), 0);
            assert_eq!(miri_rt_map_is_empty(map), 1);
            assert_eq!(miri_rt_map_get(map, 1), 0);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_grow() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            // Insert enough entries to trigger growth (initial capacity is 8, load factor 3/4)
            for i in 0..20 {
                miri_rt_map_set(map, i, i * 10);
            }
            assert_eq!(miri_rt_map_len(map), 20);

            // Verify all entries are still accessible
            for i in 0..20 {
                assert_eq!(miri_rt_map_get(map, i), i * 10);
            }

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_remove_then_reinsert() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            miri_rt_map_remove(map, 1);
            assert_eq!(miri_rt_map_get(map, 1), 0);

            // Reinsert at same key
            miri_rt_map_set(map, 1, 200);
            assert_eq!(miri_rt_map_get(map, 1), 200);
            assert_eq!(miri_rt_map_len(map), 1);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_key_at_value_at() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 10, 100);
            miri_rt_map_set(map, 20, 200);
            miri_rt_map_set(map, 30, 300);

            // Collect keys and values via key_at/value_at
            let mut keys = Vec::new();
            let mut values = Vec::new();
            for i in 0..3 {
                keys.push(miri_rt_map_key_at(map, i));
                values.push(miri_rt_map_value_at(map, i));
            }
            keys.sort();
            values.sort();

            assert_eq!(keys, vec![10, 20, 30]);
            assert_eq!(values, vec![100, 200, 300]);

            // Out of bounds returns 0
            assert_eq!(miri_rt_map_key_at(map, 3), 0);
            assert_eq!(miri_rt_map_value_at(map, 3), 0);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_rc_header() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);
            assert!(!map.is_null());

            let rc_ptr = (map as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *const usize;
            assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_null_safety() {
        unsafe {
            assert_eq!(miri_rt_map_len(std::ptr::null()), 0);
            assert_eq!(miri_rt_map_is_empty(std::ptr::null()), 1);
            miri_rt_map_set(std::ptr::null_mut(), 1, 2); // must not crash
            assert_eq!(miri_rt_map_get(std::ptr::null(), 1), 0);
            assert_eq!(miri_rt_map_contains_key(std::ptr::null(), 1), 0);
            assert_eq!(miri_rt_map_remove(std::ptr::null_mut(), 1), 0);
            miri_rt_map_clear(std::ptr::null_mut()); // must not crash
            assert_eq!(miri_rt_map_key_at(std::ptr::null(), 0), 0);
            assert_eq!(miri_rt_map_value_at(std::ptr::null(), 0), 0);
            miri_rt_map_free(std::ptr::null_mut()); // must not crash
        }
    }

    #[test]
    fn test_map_empty_operations() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            assert_eq!(miri_rt_map_is_empty(map), 1);
            assert_eq!(miri_rt_map_get(map, 42), 0);
            assert_eq!(miri_rt_map_contains_key(map, 42), 0);
            assert_eq!(miri_rt_map_remove(map, 42), 0);
            assert_eq!(miri_rt_map_key_at(map, 0), 0);
            assert_eq!(miri_rt_map_value_at(map, 0), 0);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_heavy_remove_reinsert() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            // Insert 50 entries
            for i in 0..50usize {
                miri_rt_map_set(map, i, i * 100);
            }
            assert_eq!(miri_rt_map_len(map), 50);

            // Remove even keys
            for i in (0..50usize).step_by(2) {
                assert_eq!(miri_rt_map_remove(map, i), 1);
            }
            assert_eq!(miri_rt_map_len(map), 25);

            // Verify odd keys still accessible with correct values
            for i in 0..50usize {
                if i % 2 == 0 {
                    assert_eq!(miri_rt_map_get(map, i), 0);
                    assert_eq!(miri_rt_map_contains_key(map, i), 0);
                } else {
                    assert_eq!(miri_rt_map_get(map, i), i * 100);
                    assert_eq!(miri_rt_map_contains_key(map, i), 1);
                }
            }

            // Re-insert even keys with new values
            for i in (0..50usize).step_by(2) {
                miri_rt_map_set(map, i, i * 200);
            }
            assert_eq!(miri_rt_map_len(map), 50);

            // Verify all entries
            for i in 0..50usize {
                if i % 2 == 0 {
                    assert_eq!(miri_rt_map_get(map, i), i * 200);
                } else {
                    assert_eq!(miri_rt_map_get(map, i), i * 100);
                }
            }

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_clear_then_reuse() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            for i in 0..10usize {
                miri_rt_map_set(map, i, i);
            }
            miri_rt_map_clear(map);

            // Can insert again after clear
            for i in 100..110usize {
                miri_rt_map_set(map, i, i);
            }
            assert_eq!(miri_rt_map_len(map), 10);

            // Old keys gone, new keys present
            assert_eq!(miri_rt_map_contains_key(map, 0), 0);
            assert_eq!(miri_rt_map_contains_key(map, 100), 1);
            assert_eq!(miri_rt_map_get(map, 100), 100);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_single_entry() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 42, 100);
            assert_eq!(miri_rt_map_len(map), 1);
            assert_eq!(miri_rt_map_is_empty(map), 0);
            assert_eq!(miri_rt_map_get(map, 42), 100);
            assert_eq!(miri_rt_map_contains_key(map, 42), 1);
            assert_eq!(miri_rt_map_key_at(map, 0), 42);
            assert_eq!(miri_rt_map_value_at(map, 0), 100);

            miri_rt_map_remove(map, 42);
            assert_eq!(miri_rt_map_len(map), 0);
            assert_eq!(miri_rt_map_is_empty(map), 1);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_overwrite_multiple_times() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 1, 100);
            miri_rt_map_set(map, 1, 200);
            miri_rt_map_set(map, 1, 300);
            assert_eq!(miri_rt_map_len(map), 1);
            assert_eq!(miri_rt_map_get(map, 1), 300);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_zero_value() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            // Store zero as a value — should be distinguishable from "not found"
            // (both return 0 from miri_rt_map_get, but contains_key differentiates)
            miri_rt_map_set(map, 42, 0);
            assert_eq!(miri_rt_map_get(map, 42), 0);
            assert_eq!(miri_rt_map_contains_key(map, 42), 1);
            assert_eq!(miri_rt_map_contains_key(map, 99), 0);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_iteration_after_removal() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            miri_rt_map_set(map, 10, 100);
            miri_rt_map_set(map, 20, 200);
            miri_rt_map_set(map, 30, 300);

            miri_rt_map_remove(map, 20);

            // Iterate remaining entries
            let mut keys = Vec::new();
            let mut values = Vec::new();
            for i in 0..miri_rt_map_len(map) {
                keys.push(miri_rt_map_key_at(map, i));
                values.push(miri_rt_map_value_at(map, i));
            }
            keys.sort();
            values.sort();

            assert_eq!(keys, vec![10, 30]);
            assert_eq!(values, vec![100, 300]);

            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_string_keys() {
        unsafe {
            let map = miri_rt_map_new(
                std::mem::size_of::<*const MiriString>(),
                std::mem::size_of::<usize>(),
                1, // string key kind
            );

            let key1 = Box::into_raw(Box::new(MiriString::from_str("hello")));
            let key2 = Box::into_raw(Box::new(MiriString::from_str("world")));
            let key1_dup = Box::into_raw(Box::new(MiriString::from_str("hello")));

            // Insert with string keys
            (*map).set(
                &key1 as *const *mut MiriString as *const u8,
                &100usize as *const usize as *const u8,
            );
            (*map).set(
                &key2 as *const *mut MiriString as *const u8,
                &200usize as *const usize as *const u8,
            );
            assert_eq!((*map).len, 2);

            // Look up with a duplicate key (same content, different pointer)
            let result = (*map).get(&key1_dup as *const *mut MiriString as *const u8);
            assert!(!result.is_null());
            assert_eq!(*(result as *const usize), 100);

            // Overwrite with duplicate key
            (*map).set(
                &key1_dup as *const *mut MiriString as *const u8,
                &999usize as *const usize as *const u8,
            );
            assert_eq!((*map).len, 2); // still 2 entries

            let result = (*map).get(&key1 as *const *mut MiriString as *const u8);
            assert_eq!(*(result as *const usize), 999);

            // Clean up
            let _ = Box::from_raw(key1);
            let _ = Box::from_raw(key2);
            let _ = Box::from_raw(key1_dup);
            miri_rt_map_free(map);
        }
    }

    #[test]
    fn test_map_growth_stress() {
        unsafe {
            let map = miri_rt_map_new(8, 8, 0);

            for i in 0..200usize {
                miri_rt_map_set(map, i, i * 10);
            }
            assert_eq!(miri_rt_map_len(map), 200);

            for i in 0..200usize {
                assert_eq!(miri_rt_map_get(map, i), i * 10);
            }

            miri_rt_map_free(map);
        }
    }
}
