// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
/// - `key_drop_fn`: If non-zero, called on each key pointer when that entry is
///   discarded by `remove`, `clear`, or `free`. Allows managed keys (e.g. strings)
///   to have their RC decremented when removed. Set via `miri_rt_map_set_key_drop_fn`.
#[repr(C)]
pub struct MiriMap {
    states: *mut u8,
    keys: *mut u8,
    values: *mut u8,
    pub len: usize,
    capacity: usize,
    key_size: usize,
    value_size: usize,
    key_kind: usize,
    /// Drop function for managed values: `fn(val_ptr: *mut u8)`.
    /// Zero means values are plain (no RC management on removal).
    val_drop_fn: usize,
    /// Drop function for managed keys: `fn(key_ptr: *mut u8)`.
    /// Zero means keys are plain (no RC management on removal).
    key_drop_fn: usize,
    /// Clone function for Cloneable values: `fn(val_ptr: *mut u8) -> *mut u8`.
    /// When non-zero, `miri_rt_map_clone` calls this per entry to produce an
    /// independent deep copy instead of IncRef-ing the shared pointer.
    val_clone_fn: usize,
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
                SLOT_TOMBSTONE if first_tombstone.is_none() => {
                    first_tombstone = Some(idx);
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
        let key_total = capacity.checked_mul(key_size)?;
        let keys_layout = Layout::from_size_align(key_total, 8).ok()?;
        // value_size can be 0 for value-less maps (unlikely but safe)
        let val_total = capacity.checked_mul(value_size.max(1))?;
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
        // If the key already exists and keys are managed, DecRef the old key pointer
        // before the new one is written into the slot (the new key is IncRef'd by Perceus).
        if found && self.key_drop_fn != 0 && self.key_size > 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(self.key_drop_fn);
            let old_key_addr = self.keys.add(idx * self.key_size) as *const usize;
            let old_key_ptr = *old_key_addr;
            if old_key_ptr != 0 {
                drop_fn(old_key_ptr as *mut u8);
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
            // DecRef the key being removed if managed.
            if self.key_drop_fn != 0 && self.key_size > 0 {
                let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(self.key_drop_fn);
                let key_addr = self.keys.add(idx * self.key_size) as *const usize;
                let key_ptr = *key_addr;
                if key_ptr != 0 {
                    drop_fn(key_ptr as *mut u8);
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
                let val_drop: Option<unsafe extern "C" fn(*mut u8)> =
                    if self.val_drop_fn != 0 && self.value_size > 0 {
                        Some(std::mem::transmute::<usize, unsafe extern "C" fn(*mut u8)>(
                            self.val_drop_fn,
                        ))
                    } else {
                        None
                    };
                let key_drop: Option<unsafe extern "C" fn(*mut u8)> =
                    if self.key_drop_fn != 0 && self.key_size > 0 {
                        Some(std::mem::transmute::<usize, unsafe extern "C" fn(*mut u8)>(
                            self.key_drop_fn,
                        ))
                    } else {
                        None
                    };
                if val_drop.is_some() || key_drop.is_some() {
                    for i in 0..self.capacity {
                        if *self.states.add(i) == SLOT_OCCUPIED {
                            if let Some(drop_fn) = val_drop {
                                let val_addr = self.values.add(i * self.value_size) as *const usize;
                                let val_ptr = *val_addr;
                                if val_ptr != 0 {
                                    drop_fn(val_ptr as *mut u8);
                                }
                            }
                            if let Some(drop_fn) = key_drop {
                                let key_addr = self.keys.add(i * self.key_size) as *const usize;
                                let key_ptr = *key_addr;
                                if key_ptr != 0 {
                                    drop_fn(key_ptr as *mut u8);
                                }
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
        (*map).key_drop_fn = 0;
        (*map).val_clone_fn = 0;
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

    /// Sets the drop function for managed keys.
    ///
    /// When non-zero, this function is called with the key pointer whenever an
    /// entry is discarded by `remove`, `clear`, or `free`.
    /// Must be called after `miri_rt_map_new` when keys are heap-allocated (e.g. String).
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_key_drop_fn(ptr: *mut MiriMap, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).key_drop_fn = fn_ptr;
        }
    }

    /// Sets the clone function for Cloneable values.
    ///
    /// When non-zero, `miri_rt_map_clone` calls this function on each value pointer
    /// to produce an independent deep copy instead of IncRef-ing the shared pointer.
    /// Must be called after `miri_rt_map_new` when values are Cloneable objects.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_val_clone_fn(ptr: *mut MiriMap, fn_ptr: usize) {
        if !ptr.is_null() {
            (*ptr).val_clone_fn = fn_ptr;
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
        let map = &*ptr;
        // DecRef all managed keys before freeing backing storage.
        if map.key_drop_fn != 0 && !map.states.is_null() && map.capacity > 0 {
            let drop_fn: unsafe extern "C" fn(*mut u8) = std::mem::transmute(map.key_drop_fn);
            for i in 0..map.capacity {
                if *map.states.add(i) == SLOT_OCCUPIED {
                    let key_addr = map.keys.add(i * map.key_size) as *const usize;
                    let key_ptr = *key_addr;
                    if key_ptr != 0 {
                        drop_fn(key_ptr as *mut u8);
                    }
                }
            }
        }
        // Free internal tables
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

    /// Returns a shallow clone of the map with independent RC ownership.
    ///
    /// All occupied key-value pairs are re-inserted into a fresh map. If
    /// `key_drop_fn` or `val_drop_fn` are set, the corresponding managed
    /// pointers are IncRef'd so both the original and the clone hold independent
    /// RC=1 references — preventing a double-free when either map is dropped.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_clone(ptr: *const MiriMap) -> *mut MiriMap {
        if ptr.is_null() {
            return miri_rt_map_new(0, 0, 0);
        }
        let src = &*ptr;
        let new_map = miri_rt_map_new(src.key_size, src.value_size, src.key_kind);
        if new_map.is_null() {
            return new_map;
        }
        (*new_map).val_drop_fn = src.val_drop_fn;
        (*new_map).key_drop_fn = src.key_drop_fn;
        (*new_map).val_clone_fn = src.val_clone_fn;
        if !src.states.is_null() && src.capacity > 0 {
            for i in 0..src.capacity {
                if *src.states.add(i) == SLOT_OCCUPIED {
                    let key = src.keys.add(i * src.key_size);
                    let val = src.values.add(i * src.value_size.max(1));

                    if src.val_clone_fn != 0 && src.value_size > 0 {
                        // Deep clone: call clone fn to produce an independent copy,
                        // then insert the new pointer instead of IncRef-ing the original.
                        let clone_fn: unsafe extern "C" fn(*mut u8) -> *mut u8 =
                            std::mem::transmute(src.val_clone_fn);
                        let src_val_ptr = *(val as *const usize);
                        let new_val_ptr = if src_val_ptr != 0 {
                            clone_fn(src_val_ptr as *mut u8) as usize
                        } else {
                            0usize
                        };
                        (*new_map).set(key, &new_val_ptr as *const usize as *const u8);
                    } else {
                        (*new_map).set(key, val);
                        if src.val_drop_fn != 0 && src.value_size > 0 {
                            let val_ptr = *(val as *const usize);
                            if val_ptr != 0 {
                                crate::rc::incref(val_ptr as *mut u8);
                            }
                        }
                    }

                    if src.key_drop_fn != 0 && src.key_size > 0 {
                        let key_ptr = *(key as *const usize);
                        if key_ptr != 0 {
                            crate::rc::incref(key_ptr as *mut u8);
                        }
                    }
                }
            }
        }
        new_map
    }

    /// Copy-on-Write check: if the map has more than one owner, produce an
    /// independent clone and decrement the old RC. Returns the (possibly new)
    /// pointer that the caller should now use.
    ///
    /// Invariant: the caller must treat the returned pointer as freshly owned
    /// (RC=1). The old pointer's RC is decremented inside this function and
    /// must not be used again by the caller.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_cow(ptr: *mut MiriMap) -> *mut MiriMap {
        if ptr.is_null() {
            return ptr;
        }
        let rc_ptr = (ptr as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *mut usize;
        let rc = *rc_ptr;
        if (rc as isize) < 0 || rc <= 1 {
            return ptr;
        }
        let new_ptr = miri_rt_map_clone(ptr);
        if new_ptr.is_null() {
            return ptr;
        }
        *rc_ptr -= 1;
        new_ptr
    }

    /// Decrements the RC of a managed Map element and frees it if RC reaches zero.
    ///
    /// Used as `elem_drop_fn` / `val_drop_fn` by outer collections (Array, List, Set,
    /// Map) when they remove or overwrite a Map-typed element at runtime (e.g. clear,
    /// remove, or element overwrite). Unlike the Perceus scope-exit path — which
    /// emits an inline codegen loop to DecRef managed values before calling
    /// `miri_rt_map_free` — this runtime callback has no such loop. We therefore
    /// call `val_drop_fn` on every occupied slot here, before delegating to
    /// `miri_rt_map_free`, so that managed values (e.g. List, Set, Map) nested
    /// inside the element map are correctly DecRef'd and never leaked.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_decref_element(ptr: *mut u8) {
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
            // DecRef managed values before freeing.  The Perceus inline codegen
            // loop handles this for scope-exit drops; here we must do it ourselves.
            let map = ptr as *mut MiriMap;
            if (*map).val_drop_fn != 0
                && (*map).value_size > 0
                && !(*map).states.is_null()
                && (*map).capacity > 0
            {
                let drop_fn: unsafe extern "C" fn(*mut u8) =
                    std::mem::transmute((*map).val_drop_fn);
                for i in 0..(*map).capacity {
                    if *(*map).states.add(i) == SLOT_OCCUPIED {
                        let val_addr = (*map).values.add(i * (*map).value_size) as *const usize;
                        let val_ptr = *val_addr;
                        if val_ptr != 0 {
                            drop_fn(val_ptr as *mut u8);
                        }
                    }
                }
            }
            miri_rt_map_free(ptr as *mut MiriMap);
        }
    }
} // pub mod ffi
