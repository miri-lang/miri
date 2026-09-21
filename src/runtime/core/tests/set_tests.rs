// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use by_address::{miri_rt_set_add, miri_rt_set_contains, miri_rt_set_remove};
use miri_runtime_core::set::ffi::*;

/// The three set entry points that take an element, called the way compiled
/// code calls them: by the address of the element's bytes.
///
/// A test spells an element as a value word, so each wrapper lends out that
/// word's address. The wrappers shadow the glob-imported entry points of the
/// same name, which `ffi_abi` exercises directly.
mod by_address {
    use miri_runtime_core::set::{ffi, MiriSet};

    /// # Safety
    /// `set` is a live set or null.
    pub unsafe fn miri_rt_set_add(set: *mut MiriSet, elem: usize) -> u8 {
        ffi::miri_rt_set_add(set, &elem as *const usize as *const u8)
    }

    /// # Safety
    /// `set` is a live set or null.
    pub unsafe fn miri_rt_set_contains(set: *const MiriSet, elem: usize) -> u8 {
        ffi::miri_rt_set_contains(set, &elem as *const usize as *const u8)
    }

    /// # Safety
    /// `set` is a live set or null.
    pub unsafe fn miri_rt_set_remove(set: *mut MiriSet, elem: usize) -> u8 {
        ffi::miri_rt_set_remove(set, &elem as *const usize as *const u8)
    }
}

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

/// Builds a runtime string holding `text` in an allocation of its own.
unsafe fn owned_string(text: &str) -> usize {
    miri_runtime_core::miri_rt_string_from_raw(text.as_ptr(), text.len()) as usize
}

unsafe fn release_string(ptr: usize) {
    miri_runtime_core::miri_rt_string_free(ptr as *mut miri_runtime_core::MiriString);
}

#[test]
fn test_set_of_strings_matches_equal_content_in_separate_allocations() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(set, miri_runtime_core::element_identity::BY_STRING_CONTENT);
        let stored = owned_string("pear");
        let probe = owned_string("pear");
        let other = owned_string("fig");
        assert_ne!(stored, probe, "the probe must be a separate allocation");

        assert_eq!(miri_rt_set_add(set, stored), 1);
        assert_eq!(
            miri_rt_set_add(set, probe),
            0,
            "equal content is a duplicate"
        );
        assert_eq!(miri_rt_set_len(set), 1);
        assert_eq!(miri_rt_set_contains(set, probe), 1);
        assert_eq!(miri_rt_set_contains(set, other), 0);
        assert_eq!(miri_rt_set_remove(set, probe), 1);
        assert_eq!(miri_rt_set_len(set), 0);

        miri_rt_set_free(set);
        release_string(stored);
        release_string(probe);
        release_string(other);
    }
}

#[test]
fn test_set_clone_keeps_matching_strings_by_content() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(set, miri_runtime_core::element_identity::BY_STRING_CONTENT);
        let stored = owned_string("pear");
        let probe = owned_string("pear");
        miri_rt_set_add(set, stored);

        let copy = miri_rt_set_clone(set);
        assert_eq!(miri_rt_set_contains(copy, probe), 1);
        assert_eq!(miri_rt_set_add(copy, probe), 0);

        miri_rt_set_free(set);
        miri_rt_set_free(copy);
        release_string(stored);
        release_string(probe);
    }
}

/// An element equality that treats two boxed integers as equal when they hold
/// the same number, standing in for a compiled `equals` thunk.
unsafe extern "C" fn boxed_ints_equal(a: *const u8, b: *const u8) -> u8 {
    u8::from(*(a as *const i64) == *(b as *const i64))
}

#[test]
fn test_set_with_an_equals_callback_matches_through_it() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_equals_fn(set, boxed_ints_equal as usize);
        let values: Vec<Box<i64>> = (0..40).map(|i| Box::new(i % 10)).collect();
        for value in &values {
            miri_rt_set_add(set, &**value as *const i64 as usize);
        }
        assert_eq!(
            miri_rt_set_len(set),
            10,
            "equal values across growth stay one element"
        );

        let probe = Box::new(7i64);
        let missing = Box::new(70i64);
        assert_eq!(miri_rt_set_contains(set, &*probe as *const i64 as usize), 1);
        assert_eq!(
            miri_rt_set_contains(set, &*missing as *const i64 as usize),
            0
        );
        assert_eq!(miri_rt_set_remove(set, &*probe as *const i64 as usize), 1);
        assert_eq!(miri_rt_set_len(set), 9);

        miri_rt_set_free(set);
    }
}

static RELEASED_DUPLICATES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn count_release(_elem: *mut u8) {
    RELEASED_DUPLICATES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn test_set_add_releases_the_reference_a_rejected_duplicate_donated() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_drop_fn(set, count_release as usize);
        RELEASED_DUPLICATES.store(0, std::sync::atomic::Ordering::SeqCst);

        assert_eq!(miri_rt_set_add(set, 0x1000), 1);
        assert_eq!(
            RELEASED_DUPLICATES.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert_eq!(miri_rt_set_add(set, 0x1000), 0);
        assert_eq!(
            RELEASED_DUPLICATES.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the duplicate's donated reference is released, the stored one kept"
        );
        miri_rt_set_clear(set);
        assert_eq!(
            RELEASED_DUPLICATES.load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        miri_rt_set_free(set);
    }
}

/// A boxed value in an allocation of its own, holding `value` in its first
/// eight bytes — the shape codegen gives a `Some`.
unsafe fn owned_box(value: u64) -> usize {
    let boxed = Box::into_raw(Box::new(value));
    boxed as usize
}

unsafe fn release_box(ptr: usize) {
    drop(Box::from_raw(ptr as *mut u64));
}

#[test]
fn test_set_of_optionals_matches_equal_values_in_separate_boxes() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(
            set,
            miri_runtime_core::element_identity::through_optionals(
                miri_runtime_core::element_identity::BY_BYTES,
                1,
                8,
            ),
        );
        let stored = owned_box(2);
        let probe = owned_box(2);
        let other = owned_box(3);
        assert_ne!(stored, probe, "the probe must be a separate box");

        assert_eq!(miri_rt_set_add(set, stored), 1);
        assert_eq!(
            miri_rt_set_add(set, probe),
            0,
            "an equal value in another box is a duplicate"
        );
        assert_eq!(miri_rt_set_len(set), 1);
        assert_eq!(miri_rt_set_contains(set, probe), 1);
        assert_eq!(miri_rt_set_contains(set, other), 0);

        miri_rt_set_free(set);
        release_box(stored);
        release_box(probe);
        release_box(other);
    }
}

#[test]
fn test_set_of_optionals_keeps_an_absent_value_apart_from_a_present_one() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(
            set,
            miri_runtime_core::element_identity::through_optionals(
                miri_runtime_core::element_identity::BY_BYTES,
                1,
                8,
            ),
        );
        let zero = owned_box(0);

        assert_eq!(miri_rt_set_add(set, 0), 1, "an absent value is a value");
        assert_eq!(miri_rt_set_add(set, 0), 0, "and it is one value");
        assert_eq!(
            miri_rt_set_add(set, zero),
            1,
            "a box holding zero is not the absence of a box"
        );
        assert_eq!(miri_rt_set_len(set), 2);
        assert_eq!(miri_rt_set_contains(set, 0), 1);

        miri_rt_set_free(set);
        release_box(zero);
    }
}

/// The rule applied to the boxed value is the one the kind names, so a box
/// holding a string is matched by that string's content.
#[test]
fn test_set_of_optional_strings_matches_boxed_content() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(
            set,
            miri_runtime_core::element_identity::through_optionals(
                miri_runtime_core::element_identity::BY_STRING_CONTENT,
                1,
                8,
            ),
        );
        let stored_text = owned_string("pear");
        let probe_text = owned_string("pear");
        let stored = owned_box(stored_text as u64);
        let probe = owned_box(probe_text as u64);

        assert_eq!(miri_rt_set_add(set, stored), 1);
        assert_eq!(miri_rt_set_add(set, probe), 0);
        assert_eq!(miri_rt_set_len(set), 1);
        assert_eq!(miri_rt_set_contains(set, probe), 1);

        miri_rt_set_free(set);
        release_box(stored);
        release_box(probe);
        release_string(stored_text);
        release_string(probe_text);
    }
}

/// A value narrower than its box is read at its own width: the bytes past it
/// are whatever the allocation held, and reading them would separate two equal
/// values.
#[test]
fn test_set_of_optionals_reads_a_narrow_value_at_its_own_width() {
    unsafe {
        let set = miri_rt_set_new(8);
        miri_rt_set_set_elem_kind(
            set,
            miri_runtime_core::element_identity::through_optionals(
                miri_runtime_core::element_identity::BY_BYTES,
                1,
                4,
            ),
        );
        let stored = owned_box(0x1111_1111_0000_0007);
        let probe = owned_box(0x2222_2222_0000_0007);

        assert_eq!(miri_rt_set_add(set, stored), 1);
        assert_eq!(
            miri_rt_set_add(set, probe),
            0,
            "the four bytes past the value must not be read"
        );
        assert_eq!(miri_rt_set_len(set), 1);

        miri_rt_set_free(set);
        release_box(stored);
        release_box(probe);
    }
}

/// The three element entry points called with a sixteen-byte element: twice the
/// width of a value word, which is the width the by-address ABI exists for.
mod wide {
    use miri_runtime_core::set::{ffi, MiriSet};

    /// # Safety
    /// `set` is a live set whose element size is sixteen bytes.
    pub unsafe fn add(set: *mut MiriSet, elem: i128) -> u8 {
        ffi::miri_rt_set_add(set, (&elem as *const i128).cast())
    }

    /// # Safety
    /// `set` is a live set whose element size is sixteen bytes.
    pub unsafe fn contains(set: *const MiriSet, elem: i128) -> u8 {
        ffi::miri_rt_set_contains(set, (&elem as *const i128).cast())
    }

    /// # Safety
    /// `set` is a live set whose element size is sixteen bytes.
    pub unsafe fn remove(set: *mut MiriSet, elem: i128) -> u8 {
        ffi::miri_rt_set_remove(set, (&elem as *const i128).cast())
    }
}

/// A sixteen-byte element reaches the set whole. `i128::MAX` and `-1` fill their
/// low eight bytes with the same ones and differ only above bit 63, so a set
/// handed a value word alone would fold them into one element and answer every
/// lookup for either with the other.
#[test]
fn test_set_distinguishes_sixteen_byte_elements_sharing_a_low_word() {
    unsafe {
        let set = miri_rt_set_new(16);

        assert_eq!(wide::add(set, i128::MAX), 1);
        assert_eq!(wide::add(set, -1), 1);
        assert_eq!(wide::add(set, i128::MAX), 0, "the same element twice");
        assert_eq!(miri_rt_set_len(set), 2);

        assert_eq!(wide::contains(set, i128::MAX), 1);
        assert_eq!(wide::contains(set, -1), 1);
        assert_eq!(wide::contains(set, i128::MIN), 0);

        assert_eq!(wide::remove(set, i128::MAX), 1);
        assert_eq!(miri_rt_set_len(set), 1);
        assert_eq!(wide::contains(set, i128::MAX), 0);
        assert_eq!(
            wide::contains(set, -1),
            1,
            "removing one element must leave its low-word twin"
        );

        miri_rt_set_free(set);
    }
}

/// A null element address is refused rather than read: the entry points take an
/// address from compiled code, and reading one that is not there would fault
/// before anything could report it.
#[test]
fn test_set_element_entry_points_refuse_a_null_element_address() {
    unsafe {
        let set = miri_rt_set_new(8);
        assert_eq!(miri_rt_set_add(set, 10), 1);

        assert_eq!(
            miri_runtime_core::set::ffi::miri_rt_set_add(set, std::ptr::null()),
            0
        );
        assert_eq!(
            miri_runtime_core::set::ffi::miri_rt_set_contains(set, std::ptr::null()),
            0
        );
        assert_eq!(
            miri_runtime_core::set::ffi::miri_rt_set_remove(set, std::ptr::null()),
            0
        );
        assert_eq!(miri_rt_set_len(set), 1);

        miri_rt_set_free(set);
    }
}
