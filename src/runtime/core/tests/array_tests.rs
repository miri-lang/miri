// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::array::ffi::*;

#[test]
fn test_array_new_zeroed() {
    unsafe {
        let arr = miri_rt_array_new(5, std::mem::size_of::<i32>());
        assert_eq!(miri_rt_array_len(arr), 5);

        // All elements should be zero
        for i in 0..5 {
            let p = miri_rt_array_get(arr, i);
            assert!(!p.is_null());
            assert_eq!(*(p as *const i32), 0);
        }

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_get_set() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

        let val = 42i32;
        assert_eq!(
            miri_rt_array_set(arr, 1, &val as *const i32 as *const u8),
            1
        );

        let p = miri_rt_array_get(arr, 1);
        assert_eq!(*(p as *const i32), 42);

        // Other elements still zero
        assert_eq!(*(miri_rt_array_get(arr, 0) as *const i32), 0);
        assert_eq!(*(miri_rt_array_get(arr, 2) as *const i32), 0);

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_bounds_checking() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

        // Out of bounds get returns null
        assert!(miri_rt_array_get(arr, 3).is_null());
        assert!(miri_rt_array_get(arr, 100).is_null());

        // Out of bounds set returns 0
        let val = 1i32;
        assert_eq!(
            miri_rt_array_set(arr, 3, &val as *const i32 as *const u8),
            0
        );

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_fill() {
    unsafe {
        let arr = miri_rt_array_new(4, std::mem::size_of::<i32>());

        let val = 99i32;
        miri_rt_array_fill(arr, &val as *const i32 as *const u8);

        for i in 0..4 {
            let p = miri_rt_array_get(arr, i);
            assert_eq!(*(p as *const i32), 99);
        }

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_clone() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

        let values = [10i32, 20, 30];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i32 as *const u8);
        }

        let cloned = miri_rt_array_clone(arr);
        assert_eq!(miri_rt_array_len(cloned), 3);

        for (i, v) in values.iter().enumerate() {
            let p = miri_rt_array_get(cloned, i);
            assert_eq!(*(p as *const i32), *v);
        }

        // Modifying original doesn't affect clone
        let new_val = 999i32;
        miri_rt_array_set(arr, 0, &new_val as *const i32 as *const u8);
        assert_eq!(*(miri_rt_array_get(cloned, 0) as *const i32), 10);

        miri_rt_array_free(arr);
        miri_rt_array_free(cloned);
    }
}

#[test]
fn test_array_clone_managed_elements_increfs_rc() {
    // Verify that cloning an array whose elements are RC-managed pointers
    // IncRefs each element so the clone and the original both hold RC=1
    // references independently, preventing a double-free on destruction.
    unsafe {
        // Create two managed inner lists that will serve as elements.
        let elem0 = miri_runtime_core::miri_rt_list_new(std::mem::size_of::<i32>());
        let elem1 = miri_runtime_core::miri_rt_list_new(std::mem::size_of::<i32>());

        // Both elements start at RC=1.
        let rc_ptr0 = (elem0 as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        let rc_ptr1 = (elem1 as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr0, 1);
        assert_eq!(*rc_ptr1, 1);

        // Build an outer array of pointer-sized elements.
        let arr = miri_rt_array_new(2, std::mem::size_of::<usize>());
        miri_rt_array_set(arr, 0, &(elem0 as usize) as *const usize as *const u8);
        miri_rt_array_set(arr, 1, &(elem1 as usize) as *const usize as *const u8);
        miri_rt_array_set_elem_drop_fn(
            arr,
            miri_runtime_core::miri_rt_list_decref_element as *const () as usize,
        );

        // Clone the outer array.  Elements must be IncRef'd → RC=2.
        let cloned = miri_rt_array_clone(arr);
        assert_eq!(*rc_ptr0, 2, "elem0 RC should be 2 after clone");
        assert_eq!(*rc_ptr1, 2, "elem1 RC should be 2 after clone");

        // Freeing the clone decrements each element: RC=1.
        miri_rt_array_free(cloned);
        assert_eq!(*rc_ptr0, 1, "elem0 RC should be 1 after freeing clone");
        assert_eq!(*rc_ptr1, 1, "elem1 RC should be 1 after freeing clone");

        // Freeing the original decrements to 0 and the inner lists are freed.
        // (If this double-freed, the process would abort or corrupt memory.)
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_to_list() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

        let values = [5i32, 10, 15];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i32 as *const u8);
        }

        let list = miri_rt_array_to_list(arr);
        assert_eq!(miri_runtime_core::miri_rt_list_len(list), 3);

        for (i, v) in values.iter().enumerate() {
            let p = miri_runtime_core::miri_rt_list_get(list, i);
            assert_eq!(*(p as *const i32), *v);
        }

        miri_runtime_core::miri_rt_list_free(list);
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_data_ptr() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());

        let val = 7i32;
        miri_rt_array_set(arr, 0, &val as *const i32 as *const u8);

        let data = miri_rt_array_data(arr);
        assert!(!data.is_null());
        assert_eq!(*(data as *const i32), 7);

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_empty() {
    unsafe {
        let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
        assert_eq!(miri_rt_array_len(arr), 0);
        assert!(miri_rt_array_get(arr, 0).is_null());
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort() {
    unsafe {
        let arr = miri_rt_array_new(4, std::mem::size_of::<i64>());

        let values = [30i64, 10, 20, 5];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i64 as *const u8);
        }

        miri_rt_array_sort(arr);

        assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 5);
        assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), 10);
        assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 20);
        assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 30);

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_rc_header_present() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
        assert!(!arr.is_null());

        let rc_ptr = (arr as *mut u8).sub(miri_runtime_core::rc::RC_HEADER_SIZE) as *const usize;
        assert_eq!(*rc_ptr, 1, "RC should be 1 after creation");

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_null_safety() {
    unsafe {
        assert_eq!(miri_rt_array_len(std::ptr::null()), 0);
        assert!(miri_rt_array_get(std::ptr::null(), 0).is_null());
        assert!(miri_rt_array_get_mut(std::ptr::null_mut(), 0).is_null());
        assert_eq!(
            miri_rt_array_set(std::ptr::null_mut(), 0, std::ptr::null()),
            0
        );
        miri_rt_array_fill(std::ptr::null_mut(), std::ptr::null()); // must not crash
        assert!(miri_rt_array_data(std::ptr::null()).is_null());
        miri_rt_array_sort(std::ptr::null_mut()); // must not crash
        miri_rt_array_free(std::ptr::null_mut()); // must not crash
    }
}

#[test]
fn test_array_set_null_elem() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
        assert_eq!(miri_rt_array_set(arr, 0, std::ptr::null()), 0);
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort_negative_values() {
    unsafe {
        let arr = miri_rt_array_new(5, std::mem::size_of::<i64>());
        let values = [-10i64, 5, -3, 0, 7];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i64 as *const u8);
        }

        miri_rt_array_sort(arr);

        assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), -10);
        assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), -3);
        assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 0);
        assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 5);
        assert_eq!(*(miri_rt_array_get(arr, 4) as *const i64), 7);

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort_duplicates() {
    unsafe {
        let arr = miri_rt_array_new(5, std::mem::size_of::<i64>());
        let values = [3i64, 1, 3, 2, 1];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i64 as *const u8);
        }

        miri_rt_array_sort(arr);

        assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 1);
        assert_eq!(*(miri_rt_array_get(arr, 1) as *const i64), 1);
        assert_eq!(*(miri_rt_array_get(arr, 2) as *const i64), 2);
        assert_eq!(*(miri_rt_array_get(arr, 3) as *const i64), 3);
        assert_eq!(*(miri_rt_array_get(arr, 4) as *const i64), 3);

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort_reverse_sorted() {
    unsafe {
        let arr = miri_rt_array_new(4, std::mem::size_of::<i64>());
        let values = [4i64, 3, 2, 1];
        for (i, v) in values.iter().enumerate() {
            miri_rt_array_set(arr, i, v as *const i64 as *const u8);
        }

        miri_rt_array_sort(arr);

        for i in 0..4 {
            assert_eq!(*(miri_rt_array_get(arr, i) as *const i64), (i + 1) as i64);
        }

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort_single_element() {
    unsafe {
        let arr = miri_rt_array_new(1, std::mem::size_of::<i64>());
        let val = 42i64;
        miri_rt_array_set(arr, 0, &val as *const i64 as *const u8);
        miri_rt_array_sort(arr); // must not crash
        assert_eq!(*(miri_rt_array_get(arr, 0) as *const i64), 42);
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_sort_empty() {
    unsafe {
        let arr = miri_rt_array_new(0, std::mem::size_of::<i64>());
        miri_rt_array_sort(arr); // must not crash
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_clone_empty() {
    unsafe {
        let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
        let cloned = miri_rt_array_clone(arr);
        assert_eq!(miri_rt_array_len(cloned), 0);
        miri_rt_array_free(arr);
        miri_rt_array_free(cloned);
    }
}

#[test]
fn test_array_clone_null() {
    unsafe {
        let cloned = miri_rt_array_clone(std::ptr::null());
        assert!(!cloned.is_null());
        assert_eq!(miri_rt_array_len(cloned), 0);
        miri_rt_array_free(cloned);
    }
}

#[test]
fn test_array_get_mut() {
    unsafe {
        let arr = miri_rt_array_new(3, std::mem::size_of::<i32>());
        let val = 77i32;
        miri_rt_array_set(arr, 1, &val as *const i32 as *const u8);

        let p = miri_rt_array_get_mut(arr, 1);
        assert!(!p.is_null());
        assert_eq!(*(p as *const i32), 77);

        // Write through mutable pointer
        *(p as *mut i32) = 88;
        assert_eq!(*(miri_rt_array_get(arr, 1) as *const i32), 88);

        // Out of bounds
        assert!(miri_rt_array_get_mut(arr, 3).is_null());

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_fill_all_elements() {
    unsafe {
        let arr = miri_rt_array_new(100, std::mem::size_of::<i64>());
        let val = 42i64;
        miri_rt_array_fill(arr, &val as *const i64 as *const u8);

        for i in 0..100 {
            assert_eq!(*(miri_rt_array_get(arr, i) as *const i64), 42);
        }

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_to_list_empty() {
    unsafe {
        let arr = miri_rt_array_new(0, std::mem::size_of::<i32>());
        let list = miri_rt_array_to_list(arr);
        assert_eq!(miri_runtime_core::miri_rt_list_len(list), 0);
        miri_runtime_core::miri_rt_list_free(list);
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_elem_drop_fn_called_on_free() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LAST_PTR: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
        LAST_PTR.store(p as usize, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);
        LAST_PTR.store(0, Ordering::SeqCst);

        let arr = miri_rt_array_new(3, std::mem::size_of::<usize>());

        // Fake element pointers; one null slot must be skipped.
        let e0: usize = 0xAAAA_0000;
        let e1: usize = 0; // null: should be skipped
        let e2: usize = 0xBBBB_0000;
        miri_rt_array_set(arr, 0, &e0 as *const usize as *const u8);
        miri_rt_array_set(arr, 1, &e1 as *const usize as *const u8);
        miri_rt_array_set(arr, 2, &e2 as *const usize as *const u8);

        miri_rt_array_set_elem_drop_fn(arr, counting_drop as *const () as usize);

        miri_rt_array_free(arr);

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(LAST_PTR.load(Ordering::SeqCst), 0xBBBB_0000);
    }
}

#[test]
fn test_array_free_without_drop_fn_is_noop_for_elements() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let arr = miri_rt_array_new(2, std::mem::size_of::<usize>());
        let e0: usize = 0xCCCC_0000;
        let e1: usize = 0xDDDD_0000;
        miri_rt_array_set(arr, 0, &e0 as *const usize as *const u8);
        miri_rt_array_set(arr, 1, &e1 as *const usize as *const u8);

        // Never set elem_drop_fn; defaults to 0 → no element drops.
        miri_rt_array_free(arr);

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        // Silence unused warning on counting_drop when drop_fn is 0.
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);
    }
}

#[test]
fn test_array_fill_decrefs_old_elements() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let arr = miri_rt_array_new(3, std::mem::size_of::<usize>());

        // Populate with fake managed pointers (one null slot).
        let e0: usize = 0xAAAA_0000;
        let e1: usize = 0;
        let e2: usize = 0xBBBB_0000;
        miri_rt_array_set(arr, 0, &e0 as *const usize as *const u8);
        miri_rt_array_set(arr, 1, &e1 as *const usize as *const u8);
        miri_rt_array_set(arr, 2, &e2 as *const usize as *const u8);

        // Install drop_fn only after populating, so set() doesn't fire it on zeroed slots.
        miri_rt_array_set_elem_drop_fn(arr, counting_drop as *const () as usize);

        let new_val: usize = 0xCCCC_0000;
        miri_rt_array_fill(arr, &new_val as *const usize as *const u8);

        // Two non-null old elements must have been dropped; the null slot is skipped.
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 2);

        // Clear drop_fn so free doesn't touch the fake new_val pointers.
        miri_rt_array_set_elem_drop_fn(arr, 0);
        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_fill_without_drop_fn_does_not_call_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counting_drop(_p: *mut u8) {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe {
        DROP_CALLS.store(0, Ordering::SeqCst);

        let arr = miri_rt_array_new(2, std::mem::size_of::<i32>());
        let old = 7i32;
        miri_rt_array_set(arr, 0, &old as *const i32 as *const u8);
        miri_rt_array_set(arr, 1, &old as *const i32 as *const u8);

        // No drop_fn installed — fill must not invoke anything.
        let new_val = 99i32;
        miri_rt_array_fill(arr, &new_val as *const i32 as *const u8);

        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 0);
        let _ = counting_drop as unsafe extern "C" fn(*mut u8);

        for i in 0..2 {
            assert_eq!(*(miri_rt_array_get(arr, i) as *const i32), 99);
        }

        miri_rt_array_free(arr);
    }
}

#[test]
fn test_array_to_list_null() {
    unsafe {
        let list = miri_rt_array_to_list(std::ptr::null());
        assert!(!list.is_null());
        assert_eq!(miri_runtime_core::miri_rt_list_len(list), 0);
        miri_runtime_core::miri_rt_list_free(list);
    }
}
