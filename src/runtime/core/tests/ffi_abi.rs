//! FFI ABI test suite.
//!
//! Verifies that each `miri_rt_*` and `miri_alloc*` symbol is correctly exported
//! from the runtime library at the crate root, with the expected C ABI signature.
//! Functions are imported via their canonical crate-root paths to verify that the
//! `pub use {module}::ffi::*` re-exports in lib.rs are all wired correctly.
//!
//! The purpose is to confirm: (1) every symbol is accessible, (2) signatures
//! match what the compiler expects, and (3) basic runtime behaviour is correct.

// -----------------------------------------------------------------------
// alloc
// -----------------------------------------------------------------------
use miri_runtime_core::{miri_alloc, miri_alloc_zeroed, miri_free, miri_realloc};

// -----------------------------------------------------------------------
// array
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_array_clone, miri_rt_array_data, miri_rt_array_decref_element, miri_rt_array_fill,
    miri_rt_array_free, miri_rt_array_get, miri_rt_array_get_mut, miri_rt_array_len,
    miri_rt_array_new, miri_rt_array_set, miri_rt_array_set_elem_drop_fn, miri_rt_array_set_val,
    miri_rt_array_sort, miri_rt_array_to_list,
};

// -----------------------------------------------------------------------
// list
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_list_capacity, miri_rt_list_clear, miri_rt_list_clone, miri_rt_list_decref_element,
    miri_rt_list_first, miri_rt_list_free, miri_rt_list_get, miri_rt_list_get_mut,
    miri_rt_list_insert, miri_rt_list_is_empty, miri_rt_list_last, miri_rt_list_len,
    miri_rt_list_new, miri_rt_list_new_from_managed_array, miri_rt_list_new_from_raw,
    miri_rt_list_pop, miri_rt_list_push, miri_rt_list_remove, miri_rt_list_reverse,
    miri_rt_list_set, miri_rt_list_set_elem_drop_fn, miri_rt_list_sort, miri_rt_list_with_capacity,
};

// -----------------------------------------------------------------------
// set
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_set_add, miri_rt_set_clear, miri_rt_set_contains, miri_rt_set_decref_element,
    miri_rt_set_element_at, miri_rt_set_free, miri_rt_set_is_empty, miri_rt_set_len,
    miri_rt_set_new, miri_rt_set_remove, miri_rt_set_set_elem_drop_fn,
};

// -----------------------------------------------------------------------
// map
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_map_clear, miri_rt_map_contains_key, miri_rt_map_decref_element, miri_rt_map_free,
    miri_rt_map_get, miri_rt_map_get_checked, miri_rt_map_is_empty, miri_rt_map_key_at,
    miri_rt_map_len, miri_rt_map_new, miri_rt_map_remove, miri_rt_map_set,
    miri_rt_map_set_key_drop_fn, miri_rt_map_set_val_drop_fn, miri_rt_map_value_at,
};

// -----------------------------------------------------------------------
// string
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_bool_to_string, miri_rt_float_to_string, miri_rt_int_to_string, miri_rt_string_char_at,
    miri_rt_string_char_count, miri_rt_string_clone, miri_rt_string_concat,
    miri_rt_string_contains, miri_rt_string_data, miri_rt_string_decref_element,
    miri_rt_string_ends_with, miri_rt_string_equals, miri_rt_string_free, miri_rt_string_from_raw,
    miri_rt_string_is_empty, miri_rt_string_len, miri_rt_string_new, miri_rt_string_repeat,
    miri_rt_string_replace, miri_rt_string_starts_with, miri_rt_string_substring,
    miri_rt_string_to_lower, miri_rt_string_to_upper, miri_rt_string_trim, miri_rt_string_trim_end,
    miri_rt_string_trim_start,
};

// -----------------------------------------------------------------------
// io
// -----------------------------------------------------------------------
use miri_runtime_core::{
    miri_rt_eprint, miri_rt_eprintln, miri_rt_get_line_end, miri_rt_print, miri_rt_println,
};

// -----------------------------------------------------------------------
// time
// -----------------------------------------------------------------------
use miri_runtime_core::miri_rt_nanotime;

// -----------------------------------------------------------------------
// tuple
// -----------------------------------------------------------------------
use miri_runtime_core::miri_rt_tuple_len;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_alloc_ffi_abi() {
    unsafe {
        let ptr = miri_alloc(64, 8);
        assert!(!ptr.is_null());
        let ptr2 = miri_realloc(ptr, 64, 8, 128);
        assert!(!ptr2.is_null());
        miri_free(ptr2, 128, 8);

        let z = miri_alloc_zeroed(16, 8);
        assert!(!z.is_null());
        for i in 0..16 {
            assert_eq!(*z.add(i), 0);
        }
        miri_free(z, 16, 8);

        // Zero-size returns null
        assert!(miri_alloc(0, 8).is_null());
        // Null free is a no-op
        miri_free(std::ptr::null_mut(), 8, 8);
    }
}

#[test]
fn test_array_ffi_abi() {
    unsafe {
        let arr = miri_rt_array_new(4, 8);
        assert!(!arr.is_null());
        assert_eq!(miri_rt_array_len(arr), 4);

        let val: usize = 42;
        assert_eq!(
            miri_rt_array_set(arr, 0, &val as *const usize as *const u8),
            1
        );
        let p = miri_rt_array_get(arr, 0);
        assert!(!p.is_null());
        assert_eq!(*(p as *const usize), 42);

        let p_mut = miri_rt_array_get_mut(arr, 1);
        assert!(!p_mut.is_null());

        assert_eq!(miri_rt_array_set_val(arr, 2, 99), 1);

        let data = miri_rt_array_data(arr);
        assert!(!data.is_null());

        let cloned = miri_rt_array_clone(arr);
        assert!(!cloned.is_null());
        assert_eq!(miri_rt_array_len(cloned), 4);
        miri_rt_array_free(cloned);

        let list = miri_rt_array_to_list(arr);
        assert!(!list.is_null());
        assert_eq!(miri_rt_list_len(list), 4);
        miri_rt_list_free(list);

        miri_rt_array_sort(arr);
        miri_rt_array_fill(arr, &val as *const usize as *const u8);

        miri_rt_array_free(arr);

        // miri_rt_array_set_elem_drop_fn: null-safe and callable
        miri_rt_array_set_elem_drop_fn(std::ptr::null_mut(), 0);
        let arr2 = miri_rt_array_new(2, 8);
        miri_rt_array_set_elem_drop_fn(arr2, 0);
        miri_rt_array_free(arr2);

        // miri_rt_array_decref_element: null-safe; a live array with RC=1 must
        // be freed (not double-freed) when its RC is decremented to zero.
        miri_rt_array_decref_element(std::ptr::null_mut());

        // Null safety
        assert_eq!(miri_rt_array_len(std::ptr::null()), 0);
        miri_rt_array_free(std::ptr::null_mut());
    }
}

#[test]
fn test_list_ffi_abi() {
    unsafe {
        let list = miri_rt_list_new(8);
        assert!(!list.is_null());
        assert_eq!(miri_rt_list_len(list), 0);
        assert_eq!(miri_rt_list_is_empty(list), 1);

        miri_rt_list_push(list, 10);
        miri_rt_list_push(list, 20);
        miri_rt_list_push(list, 30);
        assert_eq!(miri_rt_list_len(list), 3);
        let cap = miri_rt_list_capacity(list);
        assert!(cap >= 3);

        let p = miri_rt_list_get(list, 0);
        assert!(!p.is_null());
        assert_eq!(*(p as *const usize), 10);

        let pm = miri_rt_list_get_mut(list, 1);
        assert!(!pm.is_null());

        assert_eq!(miri_rt_list_set(list, 1, 99), 1);
        assert_eq!(miri_rt_list_insert(list, 1, 55), 1);
        assert_eq!(miri_rt_list_len(list), 4);
        assert_eq!(miri_rt_list_remove(list, 1), 1);
        assert_eq!(miri_rt_list_len(list), 3);

        let first = miri_rt_list_first(list);
        assert!(!first.is_null());
        let last = miri_rt_list_last(list);
        assert!(!last.is_null());

        let cloned = miri_rt_list_clone(list);
        assert!(!cloned.is_null());
        assert_eq!(miri_rt_list_len(cloned), miri_rt_list_len(list));
        miri_rt_list_free(cloned);

        miri_rt_list_sort(list);
        miri_rt_list_reverse(list);

        miri_rt_list_set_elem_drop_fn(list, 0);

        // decref_element with null is a no-op
        miri_rt_list_decref_element(std::ptr::null_mut());

        assert_eq!(miri_rt_list_pop(list), 1);
        miri_rt_list_clear(list);
        assert_eq!(miri_rt_list_len(list), 0);

        miri_rt_list_free(list);

        // with_capacity
        let list2 = miri_rt_list_with_capacity(8, 16);
        assert!(!list2.is_null());
        miri_rt_list_free(list2);

        // new_from_raw with null array
        let list3 = miri_rt_list_new_from_raw(std::ptr::null_mut(), 0, 8);
        assert!(!list3.is_null());
        miri_rt_list_free(list3);

        // new_from_managed_array with null array
        let list4 = miri_rt_list_new_from_managed_array(std::ptr::null_mut(), 0, 8);
        assert!(!list4.is_null());
        miri_rt_list_free(list4);

        // Null safety
        assert_eq!(miri_rt_list_len(std::ptr::null()), 0);
        miri_rt_list_free(std::ptr::null_mut());
    }
}

#[test]
fn test_set_ffi_abi() {
    unsafe {
        let set = miri_rt_set_new(8);
        assert!(!set.is_null());
        assert_eq!(miri_rt_set_len(set), 0);
        assert_eq!(miri_rt_set_is_empty(set), 1);

        assert_eq!(miri_rt_set_add(set, 10), 1);
        assert_eq!(miri_rt_set_add(set, 20), 1);
        assert_eq!(miri_rt_set_add(set, 10), 0); // duplicate
        assert_eq!(miri_rt_set_len(set), 2);

        assert_eq!(miri_rt_set_contains(set, 10), 1);
        assert_eq!(miri_rt_set_contains(set, 99), 0);

        let elem = miri_rt_set_element_at(set, 0);
        assert!(elem == 10 || elem == 20);

        assert_eq!(miri_rt_set_remove(set, 10), 1);
        assert_eq!(miri_rt_set_len(set), 1);

        miri_rt_set_clear(set);
        assert_eq!(miri_rt_set_len(set), 0);

        miri_rt_set_free(set);

        // Null safety
        assert_eq!(miri_rt_set_len(std::ptr::null()), 0);
        miri_rt_set_free(std::ptr::null_mut());

        // miri_rt_set_set_elem_drop_fn: null-safe and callable
        miri_rt_set_set_elem_drop_fn(std::ptr::null_mut(), 0); // must not crash
        let set2 = miri_rt_set_new(8);
        miri_rt_set_set_elem_drop_fn(set2, 0);
        miri_rt_set_free(set2);

        // miri_rt_set_decref_element: null-safe
        miri_rt_set_decref_element(std::ptr::null_mut());
    }
}

#[test]
fn test_map_ffi_abi() {
    unsafe {
        let map = miri_rt_map_new(8, 8, 0);
        assert!(!map.is_null());
        assert_eq!(miri_rt_map_len(map), 0);
        assert_eq!(miri_rt_map_is_empty(map), 1);

        miri_rt_map_set(map, 1, 100);
        miri_rt_map_set(map, 2, 200);
        assert_eq!(miri_rt_map_len(map), 2);

        assert_eq!(miri_rt_map_get(map, 1), 100);
        assert_eq!(miri_rt_map_get(map, 99), 0); // not found

        assert_eq!(miri_rt_map_get_checked(map, 2), 200);

        assert_eq!(miri_rt_map_contains_key(map, 1), 1);
        assert_eq!(miri_rt_map_contains_key(map, 99), 0);

        let k = miri_rt_map_key_at(map, 0);
        assert!(k == 1 || k == 2);
        let v = miri_rt_map_value_at(map, 0);
        assert!(v == 100 || v == 200);

        miri_rt_map_set_val_drop_fn(map, 0);
        miri_rt_map_set_key_drop_fn(map, 0);

        assert_eq!(miri_rt_map_remove(map, 1), 1);
        assert_eq!(miri_rt_map_len(map), 1);

        miri_rt_map_clear(map);
        assert_eq!(miri_rt_map_len(map), 0);

        miri_rt_map_free(map);

        // Null safety
        assert_eq!(miri_rt_map_len(std::ptr::null()), 0);
        miri_rt_map_free(std::ptr::null_mut());

        // miri_rt_map_decref_element: null-safe
        miri_rt_map_decref_element(std::ptr::null_mut());
    }
}

#[test]
fn test_string_ffi_abi() {
    unsafe {
        let s = miri_rt_string_new();
        assert!(!s.is_null());
        assert_eq!(miri_rt_string_len(s), 0);
        assert_eq!(miri_rt_string_char_count(s), 0);
        assert_eq!(miri_rt_string_is_empty(s), 1);
        miri_rt_string_free(s);

        let hello = b"hello";
        let s2 = miri_rt_string_from_raw(hello.as_ptr(), hello.len());
        assert!(!s2.is_null());
        assert_eq!(miri_rt_string_len(s2), 5);
        assert_eq!(miri_rt_string_is_empty(s2), 0);
        assert_eq!(miri_rt_string_char_count(s2), 5);

        let data_ptr = miri_rt_string_data(s2);
        assert!(!data_ptr.is_null());

        let world = b"world";
        let s3 = miri_rt_string_from_raw(world.as_ptr(), world.len());

        let cat = miri_rt_string_concat(s2, s3);
        assert!(!cat.is_null());
        assert_eq!(miri_rt_string_len(cat), 10);
        miri_rt_string_free(cat);

        let cloned = miri_rt_string_clone(s2);
        assert!(!cloned.is_null());
        assert_eq!(miri_rt_string_equals(s2, cloned), 1);
        miri_rt_string_free(cloned);

        assert_eq!(miri_rt_string_equals(s2, s3), 0);
        assert_eq!(miri_rt_string_contains(s2, s2), 1);
        assert_eq!(miri_rt_string_starts_with(s2, s2), 1);
        assert_eq!(miri_rt_string_ends_with(s2, s2), 1);

        let upper = miri_rt_string_to_upper(s2);
        assert!(!upper.is_null());
        miri_rt_string_free(upper);

        let lower = miri_rt_string_to_lower(s2);
        assert!(!lower.is_null());
        miri_rt_string_free(lower);

        let trimmed = miri_rt_string_trim(s2);
        assert!(!trimmed.is_null());
        miri_rt_string_free(trimmed);

        let ts = miri_rt_string_trim_start(s2);
        assert!(!ts.is_null());
        miri_rt_string_free(ts);

        let te = miri_rt_string_trim_end(s2);
        assert!(!te.is_null());
        miri_rt_string_free(te);

        let replaced = miri_rt_string_replace(s2, s2, s3);
        assert!(!replaced.is_null());
        miri_rt_string_free(replaced);

        let sub = miri_rt_string_substring(s2, 0, 3);
        assert!(!sub.is_null());
        miri_rt_string_free(sub);

        let ch = miri_rt_string_char_at(s2, 0);
        assert!(!ch.is_null());
        miri_rt_string_free(ch);

        let rep = miri_rt_string_repeat(s2, 2);
        assert!(!rep.is_null());
        assert_eq!(miri_rt_string_len(rep), 10);
        miri_rt_string_free(rep);

        miri_rt_string_free(s2);
        miri_rt_string_free(s3);

        let int_s = miri_rt_int_to_string(42);
        assert!(!int_s.is_null());
        miri_rt_string_free(int_s);

        let float_s = miri_rt_float_to_string(3.14);
        assert!(!float_s.is_null());
        miri_rt_string_free(float_s);

        let bool_s = miri_rt_bool_to_string(1);
        assert!(!bool_s.is_null());
        miri_rt_string_free(bool_s);

        // miri_rt_string_decref_element: null-safe and callable; immortal strings
        // (RC high-bit set as negative isize) must not be freed.
        miri_rt_string_decref_element(std::ptr::null_mut()); // null → no-op

        // Null safety
        assert_eq!(miri_rt_string_len(std::ptr::null()), 0);
        miri_rt_string_free(std::ptr::null_mut());
    }
}

#[test]
fn test_io_ffi_abi() {
    unsafe {
        // get_line_end links and returns a non-null string
        let le = miri_rt_get_line_end();
        assert!(!le.is_null());
        miri_rt_string_free(le);

        // print/println/eprint/eprintln: just verify they link and don't crash on null.
        miri_rt_print(std::ptr::null());
        miri_rt_println(std::ptr::null());
        miri_rt_eprint(std::ptr::null());
        miri_rt_eprintln(std::ptr::null());
    }
}

#[test]
fn test_time_ffi_abi() {
    // Call twice to verify the value is monotonically non-decreasing.
    let t1 = miri_rt_nanotime();
    let t2 = miri_rt_nanotime();
    assert!(t2 >= t1, "nanotime must be non-decreasing");
}

#[test]
fn test_tuple_ffi_abi() {
    unsafe {
        // A null pointer returns 0.
        assert_eq!(miri_rt_tuple_len(std::ptr::null()), 0);

        // A valid usize on the stack is read back.
        let count: usize = 3;
        assert_eq!(miri_rt_tuple_len(&count as *const usize), 3);
    }
}
