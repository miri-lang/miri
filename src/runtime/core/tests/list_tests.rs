// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::list::ffi::*;
use miri_runtime_core::list::*;

/// Helper: create a list and push i32 values via the internal API.
unsafe fn make_i32_list(values: &[i32]) -> *mut MiriList {
    let list = miri_rt_list_new(std::mem::size_of::<i32>());
    for val in values {
        (*list).push(val as *const i32 as *const u8);
    }
    list
}

#[test]
fn test_list_push_pop() {
    unsafe {
        let list = make_i32_list(&[10, 20, 30]);
        assert_eq!(miri_rt_list_len(list), 3);

        let mut out: i32 = 0;
        assert!((*list).pop(&mut out as *mut i32 as *mut u8));
        assert_eq!(out, 30);
        assert_eq!(miri_rt_list_len(list), 2);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_get_set() {
    unsafe {
        let list = make_i32_list(&[100, 200, 300]);

        let ptr = miri_rt_list_get(list, 1);
        assert!(!ptr.is_null());
        assert_eq!(*(ptr as *const i32), 200);

        let new_val = 999i32;
        assert!((*list).set(1, &new_val as *const i32 as *const u8));

        let ptr = miri_rt_list_get(list, 1);
        assert_eq!(*(ptr as *const i32), 999);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_insert_remove() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);

        let insert_val = 99i32;
        assert!((*list).insert(1, &insert_val as *const i32 as *const u8));
        assert_eq!(miri_rt_list_len(list), 4);

        // Verify order: [1, 99, 2, 3]
        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 1);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 99);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 2);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 3);

        // Remove at index 1
        let mut removed: i32 = 0;
        assert!((*list).remove(1, &mut removed as *mut i32 as *mut u8));
        assert_eq!(removed, 99);
        assert_eq!(miri_rt_list_len(list), 3);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_clone() {
    unsafe {
        let list = make_i32_list(&[5, 10, 15]);
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
        let list = make_i32_list(&[1, 2, 3, 4, 5]);
        miri_rt_list_reverse(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 5);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 4);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 3);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 2);
        assert_eq!(*(miri_rt_list_get(list, 4) as *const i32), 1);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 30usize);
        miri_rt_list_push(list, 10usize);
        miri_rt_list_push(list, 20usize);
        miri_rt_list_push(list, 5usize);
        miri_rt_list_sort(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 5);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 10);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 20);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const usize), 30);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort_already_sorted() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 1usize);
        miri_rt_list_push(list, 2usize);
        miri_rt_list_push(list, 3usize);
        miri_rt_list_sort(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 2);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 3);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort_single_element() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 42usize);
        miri_rt_list_sort(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 42);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_ffi_list_push() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 42);
        miri_rt_list_push(list, 100);

        assert_eq!(miri_rt_list_len(list), 2);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 42);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 100);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_null_safety() {
    unsafe {
        assert_eq!(miri_rt_list_len(std::ptr::null()), 0);
        assert_eq!(miri_rt_list_capacity(std::ptr::null()), 0);
        assert_eq!(miri_rt_list_is_empty(std::ptr::null()), 1);
        miri_rt_list_push(std::ptr::null_mut(), 42); // must not crash
        assert_eq!(miri_rt_list_pop(std::ptr::null_mut()), 0);
        assert!(miri_rt_list_get(std::ptr::null(), 0).is_null());
        assert!(miri_rt_list_get_mut(std::ptr::null_mut(), 0).is_null());
        assert_eq!(miri_rt_list_set(std::ptr::null_mut(), 0, 42), 0);
        assert_eq!(miri_rt_list_insert(std::ptr::null_mut(), 0, 42), 0);
        assert_eq!(miri_rt_list_remove(std::ptr::null_mut(), 0), 0);
        miri_rt_list_clear(std::ptr::null_mut()); // must not crash
        assert!(miri_rt_list_first(std::ptr::null()).is_null());
        assert!(miri_rt_list_last(std::ptr::null()).is_null());
        miri_rt_list_sort(std::ptr::null_mut()); // must not crash
        miri_rt_list_reverse(std::ptr::null_mut()); // must not crash
        miri_rt_list_free(std::ptr::null_mut()); // must not crash
    }
}

#[test]
fn test_list_empty_operations() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<i32>());

        assert_eq!(miri_rt_list_is_empty(list), 1);
        assert_eq!(miri_rt_list_pop(list), 0);
        assert!(miri_rt_list_first(list).is_null());
        assert!(miri_rt_list_last(list).is_null());
        assert!(miri_rt_list_get(list, 0).is_null());
        assert_eq!(miri_rt_list_set(list, 0, 42), 0);
        assert_eq!(miri_rt_list_remove(list, 0), 0);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_insert_at_beginning() {
    unsafe {
        let list = make_i32_list(&[2, 3, 4]);

        let val = 1i32;
        assert!((*list).insert(0, &val as *const i32 as *const u8));
        assert_eq!(miri_rt_list_len(list), 4);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 1);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 2);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_insert_at_end() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);

        let val = 4i32;
        assert!((*list).insert(3, &val as *const i32 as *const u8));
        assert_eq!(miri_rt_list_len(list), 4);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 4);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_insert_out_of_bounds() {
    unsafe {
        let list = make_i32_list(&[1, 2]);

        let val = 99i32;
        assert!(!(*list).insert(5, &val as *const i32 as *const u8));
        assert_eq!(miri_rt_list_len(list), 2); // unchanged

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_remove_first() {
    unsafe {
        let list = make_i32_list(&[10, 20, 30]);

        let mut removed: i32 = 0;
        assert!((*list).remove(0, &mut removed as *mut i32 as *mut u8));
        assert_eq!(removed, 10);
        assert_eq!(miri_rt_list_len(list), 2);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 20);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_remove_last() {
    unsafe {
        let list = make_i32_list(&[10, 20, 30]);

        let mut removed: i32 = 0;
        assert!((*list).remove(2, &mut removed as *mut i32 as *mut u8));
        assert_eq!(removed, 30);
        assert_eq!(miri_rt_list_len(list), 2);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_remove_out_of_bounds() {
    unsafe {
        let list = make_i32_list(&[1, 2]);

        let mut removed: i32 = 0;
        assert!(!(*list).remove(5, &mut removed as *mut i32 as *mut u8));
        assert_eq!(miri_rt_list_len(list), 2); // unchanged

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_first_last() {
    unsafe {
        let list = make_i32_list(&[10, 20, 30]);

        assert_eq!(*(miri_rt_list_first(list) as *const i32), 10);
        assert_eq!(*(miri_rt_list_last(list) as *const i32), 30);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_first_last_single() {
    unsafe {
        let list = make_i32_list(&[42]);

        assert_eq!(*(miri_rt_list_first(list) as *const i32), 42);
        assert_eq!(*(miri_rt_list_last(list) as *const i32), 42);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort_negative_values() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        // Use i64 values cast through usize FFI interface
        miri_rt_list_push(list, (-5i64) as usize);
        miri_rt_list_push(list, 3usize);
        miri_rt_list_push(list, (-1i64) as usize);
        miri_rt_list_push(list, 0usize);

        miri_rt_list_sort(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const i64), -5);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const i64), -1);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const i64), 0);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const i64), 3);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort_duplicates() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 3usize);
        miri_rt_list_push(list, 1usize);
        miri_rt_list_push(list, 3usize);
        miri_rt_list_push(list, 2usize);
        miri_rt_list_push(list, 1usize);
        miri_rt_list_sort(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 1);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 2);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const usize), 3);
        assert_eq!(*(miri_rt_list_get(list, 4) as *const usize), 3);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_sort_empty() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_sort(list); // must not crash
        assert_eq!(miri_rt_list_len(list), 0);
        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_reverse_even_count() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3, 4]);
        miri_rt_list_reverse(list);

        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 4);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const i32), 3);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const i32), 2);
        assert_eq!(*(miri_rt_list_get(list, 3) as *const i32), 1);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_reverse_single() {
    unsafe {
        let list = make_i32_list(&[42]);
        miri_rt_list_reverse(list); // must not crash
        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 42);
        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_reverse_empty() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<i32>());
        miri_rt_list_reverse(list); // must not crash
        assert_eq!(miri_rt_list_len(list), 0);
        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_clone_empty() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<i32>());
        let cloned = miri_rt_list_clone(list);
        assert_eq!(miri_rt_list_len(cloned), 0);
        miri_rt_list_free(list);
        miri_rt_list_free(cloned);
    }
}

#[test]
fn test_list_clone_null() {
    unsafe {
        let cloned = miri_rt_list_clone(std::ptr::null());
        assert!(!cloned.is_null());
        assert_eq!(miri_rt_list_len(cloned), 0);
        miri_rt_list_free(cloned);
    }
}

#[test]
fn test_list_clone_independence() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);
        let cloned = miri_rt_list_clone(list);

        // Modify original
        let val = 99i32;
        (*list).set(0, &val as *const i32 as *const u8);

        // Clone unaffected
        assert_eq!(*(miri_rt_list_get(cloned, 0) as *const i32), 1);

        miri_rt_list_free(list);
        miri_rt_list_free(cloned);
    }
}

#[test]
fn test_list_clone_managed_elements_increfs_rc() {
    // Verify that cloning a list whose elements are RC-managed pointers
    // IncRefs each element so the clone and the original hold independent
    // RC=1 references — preventing a double-free on destruction.
    //
    // miri_rt_list_free is a raw dealloc that does NOT invoke elem_drop_fn
    // (that's Perceus's job at scope exit, or miri_rt_list_decref_element
    // when the list is itself an element of another collection). So we use
    // miri_rt_list_decref_element here to simulate the real drop path.
    unsafe {
        // Create two managed inner lists (RC=1 each).
        let elem0 = miri_rt_list_new(std::mem::size_of::<i32>());
        let elem1 = miri_rt_list_new(std::mem::size_of::<i32>());

        let rc_ptr0 = (elem0 as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        let rc_ptr1 = (elem1 as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        assert_eq!(*rc_ptr0, 1);
        assert_eq!(*rc_ptr1, 1);

        // Build outer list of pointer-sized elements.
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, elem0 as usize);
        miri_rt_list_push(list, elem1 as usize);
        miri_rt_list_set_elem_drop_fn(list, miri_rt_list_decref_element as *const () as usize);

        // Clone: elements must be IncRef'd → RC=2.
        let cloned = miri_rt_list_clone(list);
        assert_eq!(*rc_ptr0, 2, "elem0 RC should be 2 after clone");
        assert_eq!(*rc_ptr1, 2, "elem1 RC should be 2 after clone");

        // Drop clone via decref_element (mirrors the Perceus outer-collection path).
        // clone RC: 1→0 → calls elem_drop_fn on each element → elem0/1 RC: 2→1.
        miri_rt_list_decref_element(cloned as *mut u8);
        assert_eq!(*rc_ptr0, 1, "elem0 RC should be 1 after dropping clone");
        assert_eq!(*rc_ptr1, 1, "elem1 RC should be 1 after dropping clone");

        // Drop original: list RC: 1→0 → elem0/1 RC: 1→0 → inner lists freed.
        miri_rt_list_decref_element(list as *mut u8);
    }
}

#[test]
fn test_list_with_capacity() {
    unsafe {
        let list = miri_rt_list_with_capacity(std::mem::size_of::<usize>(), 10);
        assert!(!list.is_null());
        assert_eq!(miri_rt_list_len(list), 0);
        assert!(miri_rt_list_capacity(list) >= 10);

        // Push should work without reallocation
        for i in 0..10usize {
            miri_rt_list_push(list, i);
        }
        assert_eq!(miri_rt_list_len(list), 10);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_clear() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);
        miri_rt_list_clear(list);
        assert_eq!(miri_rt_list_len(list), 0);
        assert_eq!(miri_rt_list_is_empty(list), 1);

        // Can push again after clear
        let val = 99i32;
        (*list).push(&val as *const i32 as *const u8);
        assert_eq!(miri_rt_list_len(list), 1);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const i32), 99);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_growth_stress() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());

        // Push many elements to trigger multiple reallocations
        for i in 0..1000usize {
            miri_rt_list_push(list, i);
        }
        assert_eq!(miri_rt_list_len(list), 1000);

        // Verify all values
        for i in 0..1000usize {
            assert_eq!(*(miri_rt_list_get(list, i) as *const usize), i);
        }

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_ffi_remove() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 10usize);
        miri_rt_list_push(list, 20usize);
        miri_rt_list_push(list, 30usize);

        assert_eq!(miri_rt_list_remove(list, 1), 1);
        assert_eq!(miri_rt_list_len(list), 2);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 10);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 30);

        // Out of bounds
        assert_eq!(miri_rt_list_remove(list, 5), 0);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_ffi_insert() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 1usize);
        miri_rt_list_push(list, 3usize);

        assert_eq!(miri_rt_list_insert(list, 1, 2usize), 1);
        assert_eq!(miri_rt_list_len(list), 3);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 1);
        assert_eq!(*(miri_rt_list_get(list, 1) as *const usize), 2);
        assert_eq!(*(miri_rt_list_get(list, 2) as *const usize), 3);

        // Out of bounds
        assert_eq!(miri_rt_list_insert(list, 10, 99usize), 0);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_ffi_set() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 1usize);
        miri_rt_list_push(list, 2usize);

        assert_eq!(miri_rt_list_set(list, 0, 99usize), 1);
        assert_eq!(*(miri_rt_list_get(list, 0) as *const usize), 99);

        // Out of bounds
        assert_eq!(miri_rt_list_set(list, 5, 99usize), 0);

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_rc_header() {
    unsafe {
        let list = miri_rt_list_new(std::mem::size_of::<i32>());
        assert!(!list.is_null());

        let rc_ptr = (list as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_elem_drop_fn_called_on_decref_element() {
    // miri_rt_list_decref_element is the runtime callback used as elem_drop_fn
    // by outer collections.  When RC -> 0 it must call elem_drop_fn on every
    // live element before freeing, mirroring what the Perceus inline codegen
    // loop does during scope-exit drops.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 0xAAAA_0000);
        miri_rt_list_push(list, 0xBBBB_0000);
        miri_rt_list_push(list, 0xCCCC_0000);

        // Install drop_fn only after populating, so mutation paths above
        // don't fire it on fresh slots.
        miri_rt_list_set_elem_drop_fn(list, counting_drop as *const () as usize);

        // miri_rt_list_decref_element decrements RC (1 -> 0) and must call
        // elem_drop_fn once per live element.
        miri_rt_list_decref_element(list as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn test_list_elem_drop_fn_not_called_on_shared_decref() {
    // When RC > 1, miri_rt_list_decref_element should decrement RC but NOT
    // call elem_drop_fn -- the list is still alive.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        miri_rt_list_push(list, 0xAAAA_0000);
        miri_rt_list_push(list, 0xBBBB_0000);
        miri_rt_list_set_elem_drop_fn(list, counting_drop as *const () as usize);

        // Bump RC to 2 manually.
        let rc_ptr = (list as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        *rc_ptr = 2;

        // First decref: RC 2 -> 1.  Must NOT call elem_drop_fn.
        miri_rt_list_decref_element(list as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(*rc_ptr, 1);

        // Second decref: RC 1 -> 0.  Must call elem_drop_fn for each element.
        miri_rt_list_decref_element(list as *mut u8);
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn test_list_elem_drop_fn_not_called_without_setting() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let list = miri_rt_list_new(std::mem::size_of::<usize>());
        // Intentionally do NOT set elem_drop_fn.
        miri_rt_list_push(list, 0xAAAA_0000);
        miri_rt_list_push(list, 0xBBBB_0000);

        miri_rt_list_decref_element(list as *mut u8); // RC 1 -> 0, frees list

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);
    }
}

#[test]
fn test_list_cow_null_returns_null() {
    unsafe {
        let result = miri_rt_list_cow(std::ptr::null_mut());
        assert!(result.is_null());
    }
}

#[test]
fn test_list_cow_unique_returns_same_pointer() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);
        let rc_ptr = (list as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr, 1);

        let cowed = miri_rt_list_cow(list);
        assert_eq!(cowed, list, "RC=1 → no copy, same pointer");
        assert_eq!(*rc_ptr, 1, "RC unchanged");

        miri_rt_list_free(list);
    }
}

#[test]
fn test_list_cow_shared_copies_and_decrefs() {
    unsafe {
        let list = make_i32_list(&[10, 20, 30]);
        let rc_ptr = (list as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        // Simulate a second owner.
        *rc_ptr = 2;

        let cowed = miri_rt_list_cow(list);
        assert_ne!(cowed, list, "RC>1 → fresh pointer");
        assert_eq!(*rc_ptr, 1, "old RC decremented");

        // New pointer must own its data and have the same values.
        let new_rc_ptr =
            (cowed as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*new_rc_ptr, 1);
        assert_eq!(miri_rt_list_len(cowed), 3);
        assert_eq!(*(miri_rt_list_get(cowed, 0) as *const i32), 10);

        miri_rt_list_free(list);
        miri_rt_list_free(cowed);
    }
}

#[test]
fn test_list_cow_immortal_returns_same_pointer() {
    unsafe {
        let list = make_i32_list(&[1, 2, 3]);
        let rc_ptr = (list as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *mut usize;
        // Immortal sentinel: negative isize encoded as usize.
        let immortal = (-1isize) as usize;
        *rc_ptr = immortal;

        let cowed = miri_rt_list_cow(list);
        assert_eq!(cowed, list, "immortal RC → no copy");
        assert_eq!(*rc_ptr, immortal, "immortal RC unchanged");

        // Restore RC=1 so test cleanup deallocs without leaving a dangling
        // "second owner" in the allocator balance.
        *rc_ptr = 1;
        miri_rt_list_free(list);
    }
}
