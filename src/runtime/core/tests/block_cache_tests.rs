// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A small block released by the runtime or by compiled code is parked on its
//! thread and handed out again for the next request of the same size class,
//! instead of going back to the system allocator.
//!
//! Every test runs on the thread the harness gives it, and the cache is per
//! thread, so no other test's allocations can take a block a test released.

use miri_runtime_core::rc::{alloc_with_rc, free_with_rc};
use miri_runtime_core::{miri_rt_block_alloc, miri_rt_block_free};

#[test]
fn released_rc_block_is_the_next_one_handed_out_for_its_size() {
    unsafe {
        let first = alloc_with_rc(40);
        assert!(!first.is_null());
        free_with_rc(first, 40);
        let second = alloc_with_rc(40);
        assert_eq!(first, second, "the released block was not reused");
        free_with_rc(second, 40);
    }
}

#[test]
fn released_rc_block_serves_a_smaller_request_of_its_class() {
    unsafe {
        let first = alloc_with_rc(40);
        free_with_rc(first, 40);
        let second = alloc_with_rc(36);
        assert_eq!(
            first, second,
            "a request of the same class missed the block"
        );
        free_with_rc(second, 36);
    }
}

#[test]
fn released_rc_block_is_not_handed_out_for_another_class() {
    unsafe {
        let small = alloc_with_rc(40);
        free_with_rc(small, 40);
        let large = alloc_with_rc(200);
        assert_ne!(small, large, "a block was handed out for a larger class");
        free_with_rc(large, 200);
        let again = alloc_with_rc(40);
        assert_eq!(small, again);
        free_with_rc(again, 40);
    }
}

#[test]
fn released_rc_block_starts_with_a_reference_count_of_one() {
    unsafe {
        let first = alloc_with_rc(40);
        *(first.sub(8) as *mut usize) = 7;
        free_with_rc(first, 40);
        let second = alloc_with_rc(40);
        assert_eq!(*(second.sub(8) as *const usize), 1);
        free_with_rc(second, 40);
    }
}

#[test]
fn released_inline_block_is_the_next_one_handed_out_for_its_size() {
    unsafe {
        let first = miri_rt_block_alloc(48);
        assert!(!first.is_null());
        miri_rt_block_free(first);
        let second = miri_rt_block_alloc(48);
        assert_eq!(first, second, "the released block was not reused");
        miri_rt_block_free(second);
    }
}

#[test]
fn inline_block_larger_than_every_class_is_allocated_and_released() {
    unsafe {
        let block = miri_rt_block_alloc(64 * 1024);
        assert!(!block.is_null());
        std::ptr::write_bytes(block.add(16), 0xA5, 64 * 1024 - 16);
        miri_rt_block_free(block);
    }
}

#[test]
fn blocks_parked_on_a_thread_that_exits_are_released_with_it() {
    let handle = std::thread::spawn(|| unsafe {
        let blocks: Vec<*mut u8> = (0..64).map(|_| alloc_with_rc(40)).collect();
        for block in blocks {
            free_with_rc(block, 40);
        }
        let inline: Vec<*mut u8> = (0..64).map(|_| miri_rt_block_alloc(48)).collect();
        for block in inline {
            miri_rt_block_free(block);
        }
    });
    assert!(handle.join().is_ok());
}

#[test]
fn block_released_on_another_thread_is_parked_there() {
    let block = unsafe { alloc_with_rc(40) } as usize;
    let handle = std::thread::spawn(move || unsafe {
        free_with_rc(block as *mut u8, 40);
        let reused = alloc_with_rc(40);
        assert_eq!(reused as usize, block);
        free_with_rc(reused, 40);
    });
    assert!(handle.join().is_ok());
}
