// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::set::ffi::*;

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

        let rc_ptr = (set as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
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
fn test_set_elem_drop_fn_called_on_free() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS_FREE: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop_free(_p: *mut u8) {
        DROP_CALLS_FREE.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS_FREE.store(0, Ordering::SeqCst);

        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_drop_fn(set, counting_drop_free as *const () as usize);

        miri_rt_set_add(set, 0xAAAA_0000);
        miri_rt_set_add(set, 0xBBBB_0000);

        miri_rt_set_free(set);

        assert_eq!(DROP_CALLS_FREE.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_set_elem_drop_fn_called_on_remove() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_drop_fn(set, counting_drop as *const () as usize);

        miri_rt_set_add(set, 0xAAAA_0000);
        miri_rt_set_add(set, 0xBBBB_0000);

        // Remove one element: drop fn should fire once.
        assert_eq!(miri_rt_set_remove(set, 0xAAAA_0000), 1);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // Remove non-existent: no extra drop.
        assert_eq!(miri_rt_set_remove(set, 0xCCCC_0000), 0);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);

        // Free remaining — one more drop.
        miri_rt_set_free(set);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_set_elem_drop_fn_called_on_clear() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_drop_fn(set, counting_drop as *const () as usize);

        miri_rt_set_add(set, 0xAAAA_0000);
        miri_rt_set_add(set, 0xBBBB_0000);
        miri_rt_set_add(set, 0xCCCC_0000);

        miri_rt_set_clear(set);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(miri_rt_set_len(set), 0);

        // Free empty set: no extra drops.
        miri_rt_set_free(set);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn test_set_free_without_drop_fn_is_noop_for_elements() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let set = miri_rt_set_new(8);
        miri_rt_set_add(set, 0xAAAA_0000);
        miri_rt_set_add(set, 0xBBBB_0000);

        // Never set elem_drop_fn — no element drops on free.
        miri_rt_set_free(set);

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);
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

#[test]
fn test_set_cow_null_returns_null() {
    unsafe {
        let result = miri_rt_set_cow(std::ptr::null_mut());
        assert!(result.is_null());
    }
}

#[test]
fn test_set_cow_unique_returns_same_pointer() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_add(set, 1);
        let rc_ptr = (set as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr, 1);

        let cowed = miri_rt_set_cow(set);
        assert_eq!(cowed, set, "RC=1 → no copy");
        assert_eq!(*rc_ptr, 1);

        miri_rt_set_free(set);
    }
}

#[test]
fn test_set_cow_shared_copies_and_decrefs() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_add(set, 10);
        miri_rt_set_add(set, 20);
        miri_rt_set_add(set, 30);
        let rc_ptr = (set as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        *rc_ptr = 2;

        let cowed = miri_rt_set_cow(set);
        assert_ne!(cowed, set, "RC>1 → fresh pointer");
        assert_eq!(*rc_ptr, 1, "old RC decremented");

        let new_rc_ptr =
            (cowed as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*new_rc_ptr, 1);
        assert_eq!(miri_rt_set_len(cowed), 3);
        assert_eq!(miri_rt_set_contains(cowed, 10), 1);
        assert_eq!(miri_rt_set_contains(cowed, 20), 1);
        assert_eq!(miri_rt_set_contains(cowed, 30), 1);

        miri_rt_set_free(set);
        miri_rt_set_free(cowed);
    }
}

#[test]
fn test_set_cow_immortal_returns_same_pointer() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_add(set, 1);
        let rc_ptr = (set as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        let immortal = (-1isize) as usize;
        *rc_ptr = immortal;

        let cowed = miri_rt_set_cow(set);
        assert_eq!(cowed, set, "immortal RC → no copy");
        assert_eq!(*rc_ptr, immortal, "immortal RC unchanged");

        *rc_ptr = 1;
        miri_rt_set_free(set);
    }
}
