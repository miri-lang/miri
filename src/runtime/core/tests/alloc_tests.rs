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

#[test]
fn test_free_null_is_noop() {
    unsafe {
        miri_free(ptr::null_mut(), 64, 8); // must not crash
    }
}

#[test]
fn test_free_zero_size_is_noop() {
    unsafe {
        let ptr = miri_alloc(64, 8);
        // Freeing with size 0 is a no-op (doesn't actually free)
        miri_free(ptr, 0, 8);
        // We can't verify the pointer is still valid, but at least it doesn't crash
        // Actually free it properly
        miri_free(ptr, 64, 8);
    }
}

#[test]
fn test_alloc_various_alignments() {
    unsafe {
        for align in [1, 2, 4, 8, 16, 32, 64] {
            let ptr = miri_alloc(128, align);
            assert!(!ptr.is_null(), "alloc with align {align} failed");
            assert_eq!(
                (ptr as usize) % align,
                0,
                "pointer not aligned to {align}"
            );
            miri_free(ptr, 128, align);
        }
    }
}

#[test]
fn test_alloc_invalid_alignment() {
    unsafe {
        // Non-power-of-two alignment
        let ptr = miri_alloc(64, 3);
        assert!(ptr.is_null());

        let ptr = miri_alloc(64, 0);
        assert!(ptr.is_null());
    }
}

#[test]
fn test_alloc_small_sizes() {
    unsafe {
        for size in [1, 2, 3, 4, 7, 8, 15, 16] {
            let ptr = miri_alloc(size, 1);
            assert!(!ptr.is_null());
            miri_free(ptr, size, 1);
        }
    }
}

#[test]
fn test_alloc_zeroed_is_actually_zeroed() {
    unsafe {
        let ptr = miri_alloc_zeroed(4096, 8);
        assert!(!ptr.is_null());
        for i in 0..4096 {
            assert_eq!(*ptr.add(i), 0, "byte {i} not zeroed");
        }
        miri_free(ptr, 4096, 8);
    }
}

#[test]
fn test_realloc_grow_preserves_data() {
    unsafe {
        let ptr = miri_alloc(16, 8);
        assert!(!ptr.is_null());

        // Write pattern
        for i in 0..16 {
            *ptr.add(i) = (i + 1) as u8;
        }

        let new_ptr = miri_realloc(ptr, 16, 8, 256);
        assert!(!new_ptr.is_null());

        // Verify original data preserved
        for i in 0..16 {
            assert_eq!(*new_ptr.add(i), (i + 1) as u8, "byte {i} not preserved");
        }

        miri_free(new_ptr, 256, 8);
    }
}

#[test]
fn test_realloc_shrink() {
    unsafe {
        let ptr = miri_alloc(256, 8);
        assert!(!ptr.is_null());

        *ptr = 42;
        let new_ptr = miri_realloc(ptr, 256, 8, 16);
        assert!(!new_ptr.is_null());
        assert_eq!(*new_ptr, 42);

        miri_free(new_ptr, 16, 8);
    }
}
