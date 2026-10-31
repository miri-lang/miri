// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Generic hash map implementation for Miri runtime.
//!
//! Implements a type-erased hash map using open addressing with linear probing.
//! Keys and values are stored as opaque byte arrays. The Miri compiler provides
//! key/value sizes and registers how keys are matched — by bytes, by string
//! content, or through the key type's `equals` — the same way a set matches
//! its elements (see [`crate::element_identity`]).

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr;

use crate::element_identity::ElementIdentity;
use crate::rc::{alloc_with_rc, free_with_rc};
use crate::slot_cursor::SlotCursor;
use std::cell::Cell;

/// State flags for hash table slots. An occupied slot's state has its high bit
/// set and carries seven bits of its key's hash below it ([`occupied_tag`]),
/// so a probe passes over a slot holding a different key without reading the
/// key in most cases.
const SLOT_EMPTY: u8 = 0;
const SLOT_TOMBSTONE: u8 = 2;
const SLOT_OCCUPIED_BIT: u8 = 0x80;

/// The state an occupied slot holding a key of hash `hash` carries.
fn occupied_tag(hash: u64) -> u8 {
    SLOT_OCCUPIED_BIT | (hash >> 57) as u8
}

/// Whether a slot in state `state` holds an entry.
fn is_occupied(state: u8) -> bool {
    state & SLOT_OCCUPIED_BIT != 0
}

/// Where a key belongs: the slot that holds it, or the one it would be
/// inserted at, and the state an entry for it carries.
#[derive(Clone, Copy)]
struct Probe {
    slot: usize,
    found: bool,
    tag: u8,
}

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
/// - `key_identity`: how two keys are recognised as the same key — see
///   [`ElementIdentity`]
/// - `val_drop_fn`: If non-zero, called on each value pointer when that entry is
///   removed by a mutation operation (`remove`, `clear`, or `set` overwriting an
///   existing key). Allows managed values (Lists, Maps, etc.) to have their RC
///   decremented on removal. Set via `miri_rt_map_set_val_drop_fn`.
/// - `key_drop_fn`: If non-zero, called on each key pointer when that entry is
///   discarded by `remove`, `clear`, or `free`. Allows managed keys (e.g. strings)
///   to have their RC decremented when removed. Set via `miri_rt_map_set_key_drop_fn`.
/// - `cursor`: where the last positional lookup landed, so iteration walks the
///   slots once in total — see [`SlotCursor`]. Zeroed bytes are an empty
///   cursor.
#[repr(C)]
pub struct MiriMap {
    states: *mut u8,
    keys: *mut u8,
    values: *mut u8,
    pub len: usize,
    capacity: usize,
    key_size: usize,
    value_size: usize,
    key_identity: ElementIdentity,
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
    cursor: SlotCursor,
    /// The slot of the key the last lookup found, while the table has not
    /// changed since: a `set` right after a `get` of the same key —
    /// `counts[k] = (counts.get(k) ?? 0) + 1` — finds it there with one
    /// comparison instead of hashing and probing again.
    last_found: Cell<Option<Probe>>,
}

const STRUCT_SIZE: usize = std::mem::size_of::<MiriMap>();

/// Raise the count of the managed reference a key or value slot holds.
///
/// # Safety
/// `slot` is null or readable for a value word.
unsafe fn incref_slot_word(slot: *const u8) {
    if slot.is_null() {
        return;
    }
    let reference = slot.cast::<usize>().read_unaligned();
    if reference != 0 {
        crate::rc::incref(reference as *mut u8);
    }
}

impl MiriMap {
    /// Raises the count of a key the map is about to hand to a caller.
    ///
    /// Everything a map returns is owned by whoever receives it: the caller
    /// releases it when its binding goes out of scope, the same as any other
    /// value a function returns. Handing out the map's own reference instead
    /// would let that release free an entry the map still holds.
    ///
    /// `key_slot` is the slot holding the key; a managed key's slot holds its
    /// reference, read unaligned because a slot promises no pointer alignment.
    unsafe fn hand_out_key(&self, key_slot: *const u8) {
        if self.key_drop_fn != 0 && self.key_size > 0 {
            incref_slot_word(key_slot);
        }
    }

    /// Raises the count of a value the map is about to hand to a caller, on the
    /// same contract as [`MiriMap::hand_out_key`].
    unsafe fn hand_out_value(&self, val_slot: *const u8) {
        if self.val_drop_fn != 0 && self.value_size > 0 {
            incref_slot_word(val_slot);
        }
    }

    /// The slot of the entry at sequential `nth`, counting occupied slots only,
    /// or `None` past the last entry.
    ///
    /// Asking for consecutive positions resumes from the previous answer, so a
    /// full walk inspects each slot once.
    unsafe fn nth_entry(&self, nth: usize) -> Option<usize> {
        let states = self.states;
        self.cursor
            .nth_occupied(self.capacity, nth, |slot| is_occupied(*states.add(slot)))
    }

    /// Finds the slot for a given key (for lookup or insertion).
    /// Returns (slot_index, found) where found indicates if the key was found.
    /// Forgets every position remembered over the table: the walk cursor and
    /// the last probe. Called whenever an entry is added, removed or moved.
    fn layout_changed(&self) {
        self.cursor.forget();
        self.last_found.set(None);
    }

    unsafe fn find_slot(&self, key: *const u8) -> Probe {
        // A key is in one slot at most, so the slot the last lookup found it
        // in answers for any key equal to the one stored there.
        if let Some(last) = self.last_found.get() {
            let stored = self.keys.add(last.slot * self.key_size);
            if self.key_identity.same(stored, key, self.key_size) {
                return last;
            }
        }
        let probe = self.probe(key);
        if probe.found {
            self.last_found.set(Some(probe));
        }
        probe
    }

    unsafe fn probe(&self, key: *const u8) -> Probe {
        if self.capacity == 0 {
            return Probe {
                slot: 0,
                found: false,
                tag: SLOT_OCCUPIED_BIT,
            };
        }
        let hash = self.key_identity.hash(key, self.key_size);
        let tag = occupied_tag(hash);
        // Capacities double from a power of two, so wrapping is a mask.
        let wrap = |slot: usize| {
            if self.capacity.is_power_of_two() {
                slot & (self.capacity - 1)
            } else {
                slot % self.capacity
            }
        };
        let mut idx = wrap(hash as usize);
        let mut first_tombstone: Option<usize> = None;

        for _ in 0..self.capacity {
            let state = *self.states.add(idx);
            if state == SLOT_EMPTY {
                // Key not found; return first tombstone if any, else this empty slot
                let slot = first_tombstone.unwrap_or(idx);
                return Probe {
                    slot,
                    found: false,
                    tag,
                };
            }
            if state == tag {
                let existing_key = self.keys.add(idx * self.key_size);
                if self.key_identity.same(existing_key, key, self.key_size) {
                    return Probe {
                        slot: idx,
                        found: true,
                        tag,
                    };
                }
            } else if state == SLOT_TOMBSTONE && first_tombstone.is_none() {
                first_tombstone = Some(idx);
            }
            idx = wrap(idx + 1);
        }

        // Table is full (shouldn't happen with proper load factor)
        let slot = first_tombstone.unwrap_or(0);
        Probe {
            slot,
            found: false,
            tag,
        }
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
        crate::guard::guard_alloc_raw(states, crate::guard::AllocKind::Buffer);
        crate::alloc_count::increment_buffer_count();
        let keys = alloc_zeroed(keys_layout);
        if keys.is_null() {
            crate::guard::guard_free_raw(states);
            dealloc(states, states_layout);
            return None;
        }
        crate::guard::guard_alloc_raw(keys, crate::guard::AllocKind::Buffer);
        crate::alloc_count::increment_buffer_count();
        let values = alloc_zeroed(values_layout);
        if values.is_null() {
            crate::guard::guard_free_raw(states);
            dealloc(states, states_layout);
            crate::guard::guard_free_raw(keys);
            dealloc(keys, keys_layout);
            return None;
        }
        crate::guard::guard_alloc_raw(values, crate::guard::AllocKind::Buffer);
        crate::alloc_count::increment_buffer_count();

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
                crate::guard::guard_free_raw(states);
                dealloc(states, layout);
            }
        }
        if !keys.is_null() {
            if let Some(key_total) = capacity.checked_mul(key_size) {
                if let Ok(layout) = Layout::from_size_align(key_total, 8) {
                    crate::guard::guard_free_raw(keys);
                    dealloc(keys, layout);
                }
            }
        }
        if !values.is_null() {
            if let Some(val_total) = capacity.checked_mul(value_size.max(1)) {
                if let Ok(layout) = Layout::from_size_align(val_total, 8) {
                    crate::guard::guard_free_raw(values);
                    dealloc(values, layout);
                }
            }
        }
    }

    /// Grows the table and rehashes all entries.
    unsafe fn grow(&mut self) {
        crate::rc_stats::count_realloc();
        // Security Invariant: Guard against capacity multiplication overflow
        let new_capacity = if self.capacity == 0 {
            INITIAL_CAPACITY
        } else {
            self.capacity
                .checked_mul(2)
                .unwrap_or_else(|| std::process::abort())
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
        self.layout_changed();

        for i in 0..old_capacity {
            if is_occupied(*old_states.add(i)) {
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
        let Probe {
            slot: idx,
            found,
            tag,
        } = self.find_slot(key);

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
            *self.states.add(idx) = tag;
            self.len += 1;
            self.layout_changed();
        }
    }

    /// Sets a key-value pair, growing the table if necessary.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn set(&mut self, key: *const u8, value: *const u8) {
        // Security Invariant: Use checked multiplication to prevent integer overflow during load factor checks
        let need_grow = self.capacity == 0
            || match (
                self.len.saturating_add(1).checked_mul(LOAD_FACTOR_DEN),
                self.capacity.checked_mul(LOAD_FACTOR_NUM),
            ) {
                (Some(l), Some(c)) => l > c,
                _ => true,
            };
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
        let Probe {
            slot: idx, found, ..
        } = self.find_slot(key);
        if found {
            self.values.add(idx * self.value_size)
        } else {
            ptr::null()
        }
    }

    /// The slot holding `key`, or `None` if the map does not hold it.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn slot_of(&self, key: *const u8) -> Option<usize> {
        if self.capacity == 0 || self.len == 0 {
            return None;
        }
        let probe = self.find_slot(key);
        probe.found.then_some(probe.slot)
    }

    /// The value in `slot`, or `None` unless the slot holds an entry.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn value_in_slot(&self, slot: i64) -> Option<*const u8> {
        let slot = usize::try_from(slot)
            .ok()
            .filter(|&slot| slot < self.capacity)?;
        is_occupied(*self.states.add(slot))
            .then(|| self.values.add(slot * self.value_size).cast_const())
    }

    /// Removes a key-value pair. Returns true if the key was found and removed.
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn remove(&mut self, key: *const u8) -> bool {
        if self.capacity == 0 || self.len == 0 {
            return false;
        }
        let Probe {
            slot: idx, found, ..
        } = self.find_slot(key);
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
            self.layout_changed();
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
        self.find_slot(key).found
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
                        if is_occupied(*self.states.add(i)) {
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
        self.layout_changed();
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
    use crate::element_bytes::{read_slot, require_fits_slot, with_slot_bytes};
    use crate::guard;
    use std::ptr;

    /// Creates a new empty map with the given key/value sizes and key kind.
    ///
    /// `key_kind` is one of the kinds in [`crate::element_identity`].
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
        let mut key_identity = ElementIdentity::BYTES;
        key_identity.set_kind(key_kind);
        // The block may be one a released value left behind: every field is
        // written, never left to the allocator.
        map.write(MiriMap {
            states: ptr::null_mut(),
            keys: ptr::null_mut(),
            values: ptr::null_mut(),
            len: 0,
            capacity: 0,
            key_size,
            value_size,
            key_identity,
            val_drop_fn: 0,
            key_drop_fn: 0,
            val_clone_fn: 0,
            cursor: SlotCursor::default(),
            last_found: Cell::new(None),
        });
        map
    }

    /// Returns the number of entries in the map.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_len(ptr: *const MiriMap) -> usize {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return 0;
        }
        (*ptr).len
    }

    /// Returns true (1) if the map is empty, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_is_empty(ptr: *const MiriMap) -> u8 {
        guard::guard_check(ptr as *mut u8);
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
    /// Key and value are each handed over by address: `key` points at its
    /// `key_payload` bytes and `value` at its `value_payload` bytes, each laid
    /// out at the full width of its slot (see [`crate::element_bytes`]), so an
    /// entry of any width arrives whole. A managed key or value is the
    /// reference written at that address, donated to the map.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set(
        ptr: *mut MiriMap,
        key: *const u8,
        key_payload: usize,
        value: *const u8,
        value_payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        if value.is_null() {
            return;
        }
        let Some(map) = key_receiver(ptr, key, key_payload) else {
            return;
        };
        require_fits_slot(value_payload, map.value_size);
        let value_size = map.value_size;
        with_slot_bytes(key, key_payload, map.key_size, |key| {
            with_slot_bytes(value, value_payload, value_size, |value| {
                map.set(key, value)
            })
        });
    }

    /// Hands the value for a key, handed over as `miri_rt_map_set` takes it,
    /// to the caller's `out_payload`-byte storage at `out`.
    ///
    /// A key the map does not hold writes a zero value.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_get(
        ptr: *const MiriMap,
        key: *const u8,
        key_payload: usize,
        out: *mut u8,
        out_payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        let Some(map) = key_receiver(ptr as *mut MiriMap, key, key_payload) else {
            read_slot(out, ptr::null(), out_payload, 0);
            return;
        };
        let result = with_slot_bytes(key, key_payload, map.key_size, |key| map.get(key));
        if !result.is_null() {
            map.hand_out_value(result);
        }
        read_slot(out, result, out_payload, map.value_size);
    }

    /// The slot holding a key, handed over as `miri_rt_map_set` takes it, or
    /// -1 if the map does not hold the key. With
    /// [`miri_rt_map_value_in_slot`] it answers a lookup with one probe where
    /// asking `contains_key` and then `get` makes two.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_slot_of(
        ptr: *const MiriMap,
        key: *const u8,
        key_payload: usize,
    ) -> i64 {
        guard::guard_check(ptr as *mut u8);
        let Some(map) = key_receiver(ptr as *mut MiriMap, key, key_payload) else {
            return -1;
        };
        with_slot_bytes(key, key_payload, map.key_size, |key| map.slot_of(key))
            .and_then(|slot| i64::try_from(slot).ok())
            .unwrap_or(-1)
    }

    /// Hands the value in `slot`, as [`miri_rt_map_slot_of`] found it, to the
    /// caller's `out_payload`-byte storage at `out`, owned as
    /// [`miri_rt_map_get`] hands it. A slot past the table, or one that holds
    /// no entry because the map changed since, writes a zero value.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_value_in_slot(
        ptr: *const MiriMap,
        slot: i64,
        out: *mut u8,
        out_payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        let Some(map) = ptr.as_ref() else {
            read_slot(out, ptr::null(), out_payload, 0);
            return;
        };
        let Some(value) = map.value_in_slot(slot) else {
            read_slot(out, ptr::null(), out_payload, 0);
            return;
        };
        map.hand_out_value(value);
        read_slot(out, value, out_payload, map.value_size);
    }

    /// Hands the value for a key, handed over as `miri_rt_map_set` takes it,
    /// to the caller's `out_payload`-byte storage at `out`, aborting if the key
    /// is not found.
    ///
    /// Used for direct map indexing (`m[key]`). For safe access, use `m.get(key)`
    /// which returns an Option.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_get_checked(
        ptr: *const MiriMap,
        key: *const u8,
        key_payload: usize,
        out: *mut u8,
        out_payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            eprintln!("Runtime error: map index on null map");
            std::process::abort();
        }
        if key.is_null() {
            eprintln!("Runtime error: map index with no key");
            std::process::abort();
        }
        let Some(map) = key_receiver(ptr as *mut MiriMap, key, key_payload) else {
            std::process::abort();
        };
        let result = with_slot_bytes(key, key_payload, map.key_size, |key| map.get(key));
        if result.is_null() {
            eprintln!("Runtime error: map key not found");
            std::process::abort();
        }
        // Indexing reads through to the entry the map still owns: `m[k]` is
        // consumed in place by the expression around it, which never releases
        // what it read. Raising the count here would strand it instead.
        read_slot(out, result, out_payload, map.value_size);
    }

    /// Returns true (1) if the map contains the given key, handed over as
    /// `miri_rt_map_set` takes it.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_contains_key(
        ptr: *const MiriMap,
        key: *const u8,
        key_payload: usize,
    ) -> u8 {
        guard::guard_check(ptr as *mut u8);
        let Some(map) = key_receiver(ptr as *mut MiriMap, key, key_payload) else {
            return 0;
        };
        with_slot_bytes(key, key_payload, map.key_size, |key| {
            u8::from(map.contains_key(key))
        })
    }

    /// Removes the entry with the given key, handed over as `miri_rt_map_set`
    /// takes it.
    ///
    /// Returns true (1) if the key was found and removed, false (0) otherwise.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_remove(
        ptr: *mut MiriMap,
        key: *const u8,
        key_payload: usize,
    ) -> u8 {
        guard::guard_check(ptr as *mut u8);
        let Some(map) = key_receiver(ptr, key, key_payload) else {
            return 0;
        };
        let key_size = map.key_size;
        with_slot_bytes(key, key_payload, key_size, |key| u8::from(map.remove(key)))
    }

    /// The map a key entry point works on, or `None` when there is no map or no
    /// key to work with.
    ///
    /// A key whose width does not fit the map's key slots is a compiler defect
    /// that would store or look up part of a key, so the process is aborted.
    unsafe fn key_receiver<'a>(
        ptr: *mut MiriMap,
        key: *const u8,
        key_payload: usize,
    ) -> Option<&'a mut MiriMap> {
        if ptr.is_null() || key.is_null() {
            return None;
        }
        let map = &mut *ptr;
        require_fits_slot(key_payload, map.key_size);
        Some(map)
    }

    /// Clears all entries from the map.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_clear(ptr: *mut MiriMap) {
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
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
        guard::guard_check(ptr as *mut u8);
        if !ptr.is_null() {
            (*ptr).val_clone_fn = fn_ptr;
        }
    }

    /// Sets the key kind, selecting how keys are hashed and compared.
    ///
    /// A map literal passes its key kind to `miri_rt_map_new`, but a map built
    /// by an empty constructor has no key operands to derive it from, so codegen
    /// supplies it here from the declared key type.
    ///
    /// # Safety
    /// - `ptr` must be a valid pointer to a `MiriMap`, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_key_kind(ptr: *mut MiriMap, key_kind: usize) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return;
        }
        (*ptr).key_identity.set_kind(key_kind);
    }

    /// Matches keys through `fn_ptr`, an
    /// [`crate::element_identity::ElementEqualsFn`] the compiler generates
    /// from the key type's own `equals`.
    ///
    /// Registered before the first entry is stored, since every stored key sits
    /// where the rule in force when it arrived hashed it.
    ///
    /// # Safety
    /// - `ptr` must be a valid pointer to a `MiriMap`, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_key_equals_fn(ptr: *mut MiriMap, fn_ptr: usize) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return;
        }
        (*ptr).key_identity.set_equals_fn(fn_ptr);
    }

    /// Hashes keys through `fn_ptr`, an
    /// [`crate::element_identity::ElementHashFn`] the compiler generates from
    /// the key type's `hash`, consistent with the registered equality.
    ///
    /// Registered before the first entry is stored, on the same contract as
    /// [`miri_rt_map_set_key_equals_fn`].
    ///
    /// # Safety
    /// - `ptr` must be a valid pointer to a `MiriMap`, or null.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_set_key_hash_fn(ptr: *mut MiriMap, fn_ptr: usize) {
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return;
        }
        (*ptr).key_identity.set_hash_fn(fn_ptr);
    }

    /// Hands the key at the nth occupied slot (0-based sequential index) to
    /// the caller's `payload`-byte storage at `out`.
    ///
    /// This enables iteration over map keys via `element_at`. An index past
    /// the last entry writes a zero key.
    ///
    /// A managed key is handed out with its count already raised: the caller
    /// owns what it receives and releases it when the binding goes out of
    /// scope. Returning the map's own reference instead would let one pass of a
    /// loop free a key the map still holds.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_key_at(
        ptr: *const MiriMap,
        nth: usize,
        out: *mut u8,
        payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        let Some((map, slot)) = nth_slot(ptr, nth) else {
            read_slot(out, ptr::null(), payload, 0);
            return;
        };
        let key = map.keys.add(slot * map.key_size);
        map.hand_out_key(key);
        read_slot(out, key, payload, map.key_size);
    }

    /// Hands the value at the nth occupied slot (0-based sequential index) to
    /// the caller's `payload`-byte storage at `out`.
    ///
    /// This enables `for k, v in map` iteration. A managed value is handed out
    /// owned, on the same contract as [`MiriMap::miri_rt_map_key_at`]. An index
    /// past the last entry writes a zero value.
    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn miri_rt_map_value_at(
        ptr: *const MiriMap,
        nth: usize,
        out: *mut u8,
        payload: usize,
    ) {
        guard::guard_check(ptr as *mut u8);
        let Some((map, slot)) = nth_slot(ptr, nth) else {
            read_slot(out, ptr::null(), payload, 0);
            return;
        };
        let value = map.values.add(slot * map.value_size);
        map.hand_out_value(value);
        read_slot(out, value, payload, map.value_size);
    }

    /// The map behind `ptr` and the slot of its entry at sequential `nth`, or
    /// `None` for a null map or an index past the last entry.
    unsafe fn nth_slot<'a>(ptr: *const MiriMap, nth: usize) -> Option<(&'a MiriMap, usize)> {
        let map = ptr.as_ref()?;
        Some((map, map.nth_entry(nth)?))
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
                if is_occupied(*map.states.add(i)) {
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
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return miri_rt_map_new(0, 0, 0);
        }
        let src = &*ptr;
        let new_map = miri_rt_map_new(src.key_size, src.value_size, 0);
        if new_map.is_null() {
            return new_map;
        }
        (*new_map).key_identity = src.key_identity;
        (*new_map).val_drop_fn = src.val_drop_fn;
        (*new_map).key_drop_fn = src.key_drop_fn;
        (*new_map).val_clone_fn = src.val_clone_fn;
        if !src.states.is_null() && src.capacity > 0 {
            for i in 0..src.capacity {
                if is_occupied(*src.states.add(i)) {
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
        crate::guard::resolve_tracking_state();
        guard::guard_check(ptr as *mut u8);
        if ptr.is_null() {
            return ptr;
        }
        let rc_ptr = (ptr as *mut u8).sub(crate::rc::RC_HEADER_SIZE) as *mut usize;
        let rc = *rc_ptr as isize;
        if rc == 1 {
            return ptr;
        }
        // Shared, or immortal — held by every reader for the whole run: the
        // write goes to a copy only the caller holds, and an immortal
        // original keeps its count.
        let new_ptr = miri_rt_map_clone(ptr);
        crate::rc_stats::count_cow_copy();
        if new_ptr.is_null() {
            return ptr;
        }
        if rc > 1 {
            *rc_ptr -= 1;
        }
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
                    if is_occupied(*(*map).states.add(i)) {
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

#[cfg(test)]
mod walk_tests {
    use super::ffi::*;
    use super::MiriMap;
    use crate::slot_cursor::take_probes;

    const WORD: usize = std::mem::size_of::<usize>();

    /// A map of plain word keys `0..len`, each holding twice its key.
    unsafe fn map_of(len: usize) -> *mut MiriMap {
        let map = miri_rt_map_new(WORD, WORD, 0);
        for key in 0..len {
            set(map, key, key * 2);
        }
        map
    }

    unsafe fn set(map: *mut MiriMap, key: usize, value: usize) {
        miri_rt_map_set(map, word_addr(&key), WORD, word_addr(&value), WORD);
    }

    unsafe fn key_at(map: *const MiriMap, nth: usize) -> usize {
        let mut key = 0usize;
        miri_rt_map_key_at(map, nth, &mut key as *mut usize as *mut u8, WORD);
        key
    }

    unsafe fn value_at(map: *const MiriMap, nth: usize) -> usize {
        let mut value = 0usize;
        miri_rt_map_value_at(map, nth, &mut value as *mut usize as *mut u8, WORD);
        value
    }

    fn word_addr(word: &usize) -> *const u8 {
        word as *const usize as *const u8
    }

    #[test]
    fn full_walk_of_a_large_map_probes_linearly() {
        let _balance = crate::rc::balance_guard();
        unsafe {
            let len = 100_000;
            let map = map_of(len);
            let capacity = (*map).capacity;
            take_probes();
            let mut seen = vec![false; len];
            for nth in 0..len {
                let key = key_at(map, nth);
                assert_eq!(value_at(map, nth), key * 2);
                seen[key] = true;
            }
            let probes = take_probes();
            assert!(seen.iter().all(|&s| s), "every key is walked once");
            assert!(
                probes <= capacity + 2 * len,
                "a walk of {len} entries over {capacity} slots probed {probes} slots"
            );
            miri_rt_map_free(map);
        }
    }

    #[test]
    fn removal_mid_walk_counts_positions_in_the_changed_map() {
        let _balance = crate::rc::balance_guard();
        unsafe {
            let map = map_of(50);
            let fifth = key_at(map, 5);
            let sixth = key_at(map, 6);
            miri_rt_map_remove(map, word_addr(&fifth), WORD);
            assert_eq!(
                key_at(map, 5),
                sixth,
                "the entry after a removed one moves up"
            );
            miri_rt_map_free(map);
        }
    }

    #[test]
    fn insertion_mid_walk_counts_positions_in_the_changed_map() {
        let _balance = crate::rc::balance_guard();
        unsafe {
            let map = map_of(20);
            key_at(map, 10);
            for key in 1_000..1_100 {
                set(map, key, 0);
            }
            let walked: Vec<_> = (0..120).map(|nth| key_at(map, nth)).collect();
            let mut sorted = walked.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                120,
                "a walk after growth sees each entry once"
            );
            miri_rt_map_free(map);
        }
    }
}
