// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::map::ffi::*;
use miri_runtime_core::string::MiriString;

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

        let rc_ptr = (map as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
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

#[test]
fn test_map_val_drop_fn_called_on_decref_element() {
    // miri_rt_map_decref_element is the runtime callback used as elem_drop_fn
    // or val_drop_fn by outer collections.  When RC → 0 it must call val_drop_fn
    // on every occupied slot before freeing, mirroring what the Perceus inline
    // codegen loop does during scope-exit drops.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_val_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 1, 0xAAAA_0000);
        miri_rt_map_set(map, 2, 0xBBBB_0000);

        // miri_rt_map_decref_element decrements RC (1 → 0) and must call
        // val_drop_fn once per occupied slot.
        miri_rt_map_decref_element(map as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_map_val_drop_fn_called_on_clear() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_val_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 1, 0xAAAA_0000);
        miri_rt_map_set(map, 2, 0xBBBB_0000);
        miri_rt_map_set(map, 3, 0xCCCC_0000);

        miri_rt_map_clear(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(miri_rt_map_len(map), 0);

        // Free the now-empty map: no extra drops.
        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn test_map_val_drop_fn_called_on_overwrite() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_val_drop_fn(map, counting_drop as *const () as usize);

        // First insert: no old value → no drop.
        miri_rt_map_set(map, 1, 0xAAAA_0000);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);

        // Overwrite same key: old value must be dropped once.
        miri_rt_map_set(map, 1, 0xBBBB_0000);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // miri_rt_map_free does NOT call val_drop_fn — the Perceus inline
        // codegen loop handles values at scope exit.  Only the mutation
        // operations (set overwrite, remove, clear, decref_element) call it.
        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn test_map_val_drop_fn_not_called_on_shared_decref() {
    // When RC > 1, miri_rt_map_decref_element should decrement RC but NOT
    // call val_drop_fn — the map is still alive.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_val_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 1, 0xAAAA_0000);
        miri_rt_map_set(map, 2, 0xBBBB_0000);

        // Bump RC to 2 manually.
        let rc_ptr = (map as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        *rc_ptr = 2;

        // First decref: RC 2 → 1. Must NOT call val_drop_fn.
        miri_rt_map_decref_element(map as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(*rc_ptr, 1);

        // Second decref: RC 1 → 0. Must call val_drop_fn for each occupied slot.
        miri_rt_map_decref_element(map as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_map_val_drop_fn_not_called_without_setting() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        // Intentionally do NOT set val_drop_fn.
        miri_rt_map_set(map, 1, 0xAAAA_0000);
        miri_rt_map_set(map, 2, 0xBBBB_0000);

        miri_rt_map_remove(map, 1);
        miri_rt_map_clear(map);
        // Re-populate and use decref_element path too.
        miri_rt_map_set(map, 3, 0xCCCC_0000);
        miri_rt_map_decref_element(map as *mut u8); // RC 1 → 0, frees map

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);
    }
}

#[test]
fn test_map_key_drop_fn_called_on_remove() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_key_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 0xAAAA_0000, 1);
        miri_rt_map_set(map, 0xBBBB_0000, 2);

        // Remove one key: drop fn fires once.
        assert_eq!(miri_rt_map_remove(map, 0xAAAA_0000), 1);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // Remove non-existent key: no extra drop.
        assert_eq!(miri_rt_map_remove(map, 0xCCCC_0000), 0);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // Free remaining map — one more drop for the remaining key.
        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_map_key_drop_fn_called_on_clear() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_key_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 0xAAAA_0000, 1);
        miri_rt_map_set(map, 0xBBBB_0000, 2);
        miri_rt_map_set(map, 0xCCCC_0000, 3);

        miri_rt_map_clear(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(miri_rt_map_len(map), 0);

        // Free empty map: no extra drops.
        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn test_map_key_drop_fn_called_on_free() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_key_drop_fn(map, counting_drop as *const () as usize);

        miri_rt_map_set(map, 0xAAAA_0000, 1);
        miri_rt_map_set(map, 0xBBBB_0000, 2);

        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_map_key_drop_fn_not_called_without_setting() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        // Intentionally do NOT set key_drop_fn.
        miri_rt_map_set(map, 0xAAAA_0000, 1);
        miri_rt_map_set(map, 0xBBBB_0000, 2);

        miri_rt_map_remove(map, 0xAAAA_0000);
        miri_rt_map_clear(map);
        miri_rt_map_free(map);

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);
    }
}

#[test]
fn test_map_key_drop_fn_called_on_overwrite() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_key_drop_fn(map, counting_drop as *const () as usize);

        // First insert: no old key → no drop.
        miri_rt_map_set(map, 0xAAAA_0000, 1);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);

        // Overwrite same key: old key must be dropped once.
        miri_rt_map_set(map, 0xAAAA_0000, 2);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // Free the map: one remaining key drop.
        miri_rt_map_free(map);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_map_clear_calls_both_drop_fns() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static VAL_DROPS: AtomicUsize = AtomicUsize::new(0);
    static KEY_DROPS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn val_drop(_p: *mut u8) {
        VAL_DROPS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe extern "C" fn key_drop(_p: *mut u8) {
        KEY_DROPS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        VAL_DROPS.store(0, Ordering::SeqCst);
        KEY_DROPS.store(0, Ordering::SeqCst);

        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set_val_drop_fn(map, val_drop as *const () as usize);
        miri_rt_map_set_key_drop_fn(map, key_drop as *const () as usize);

        miri_rt_map_set(map, 0xAAAA_0000, 0x1111_0000);
        miri_rt_map_set(map, 0xBBBB_0000, 0x2222_0000);
        miri_rt_map_set(map, 0xCCCC_0000, 0x3333_0000);

        miri_rt_map_clear(map);
        assert_eq!(
            VAL_DROPS.load(Ordering::SeqCst),
            3,
            "val_drop_fn must fire once per slot"
        );
        assert_eq!(
            KEY_DROPS.load(Ordering::SeqCst),
            3,
            "key_drop_fn must fire once per slot"
        );
        assert_eq!(miri_rt_map_len(map), 0);

        miri_rt_map_free(map);
        assert_eq!(VAL_DROPS.load(Ordering::SeqCst), 3);
        assert_eq!(KEY_DROPS.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn test_map_cow_null_returns_null() {
    unsafe {
        let result = miri_rt_map_cow(std::ptr::null_mut());
        assert!(result.is_null());
    }
}

#[test]
fn test_map_cow_unique_returns_same_pointer() {
    unsafe {
        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set(map, 1, 100);
        let rc_ptr = (map as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr, 1);

        let cowed = miri_rt_map_cow(map);
        assert_eq!(cowed, map, "RC=1 → no copy");
        assert_eq!(*rc_ptr, 1);

        miri_rt_map_free(map);
    }
}

#[test]
fn test_map_cow_shared_copies_and_decrefs() {
    unsafe {
        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set(map, 1, 100);
        miri_rt_map_set(map, 2, 200);
        let rc_ptr = (map as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        *rc_ptr = 2;

        let cowed = miri_rt_map_cow(map);
        assert_ne!(cowed, map, "RC>1 → fresh pointer");
        assert_eq!(*rc_ptr, 1, "old RC decremented");

        let new_rc_ptr =
            (cowed as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*new_rc_ptr, 1);
        assert_eq!(miri_rt_map_len(cowed), 2);
        assert_eq!(miri_rt_map_get(cowed, 1), 100);
        assert_eq!(miri_rt_map_get(cowed, 2), 200);

        miri_rt_map_free(map);
        miri_rt_map_free(cowed);
    }
}

#[test]
fn test_map_cow_immortal_returns_same_pointer() {
    unsafe {
        let map = miri_rt_map_new(8, 8, 0);
        miri_rt_map_set(map, 1, 100);
        let rc_ptr = (map as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        let immortal = (-1isize) as usize;
        *rc_ptr = immortal;

        let cowed = miri_rt_map_cow(map);
        assert_eq!(cowed, map, "immortal RC → no copy");
        assert_eq!(*rc_ptr, immortal, "immortal RC unchanged");

        *rc_ptr = 1;
        miri_rt_map_free(map);
    }
}
