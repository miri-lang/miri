use miri_runtime_core::alloc::{miri_alloc, miri_alloc_zeroed, miri_free, miri_realloc};
use std::ptr;

#[test]
fn test_alloc_and_free() {
    unsafe {
        let ptr = miri_alloc(1024, 8);
        assert!(!ptr.is_null());
        miri_free(ptr, 1024, 8);
    }
}

#[test]
fn test_alloc_zeroed() {
    unsafe {
        let ptr = miri_alloc_zeroed(64, 8);
        assert!(!ptr.is_null());
        for i in 0..64 {
            assert_eq!(*ptr.add(i), 0);
        }
        miri_free(ptr, 64, 8);
    }
}

#[test]
fn test_zero_size_returns_null() {
    unsafe {
        let ptr = miri_alloc(0, 8);
        assert!(ptr.is_null());
    }
}

#[test]
fn test_realloc() {
    unsafe {
        let ptr = miri_alloc(64, 8);
        assert!(!ptr.is_null());

        // Write some data
        *ptr = 42;

        let new_ptr = miri_realloc(ptr, 64, 8, 128);
        assert!(!new_ptr.is_null());
        assert_eq!(*new_ptr, 42); // Data preserved

        miri_free(new_ptr, 128, 8);
    }
}

#[test]
fn test_realloc_null() {
    unsafe {
        let ptr = miri_realloc(ptr::null_mut(), 0, 8, 64);
        assert!(!ptr.is_null());
        miri_free(ptr, 64, 8);
    }
}

#[test]
fn test_realloc_to_zero() {
    unsafe {
        let ptr = miri_alloc(64, 8);
        assert!(!ptr.is_null());
        let new_ptr = miri_realloc(ptr, 64, 8, 0);
        assert!(new_ptr.is_null());
    }
}
