// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A thread's parked blocks go back to the system when the thread exits. The
//! binary's allocator records every block released at the cache's alignment,
//! so the test can check each block the thread parked was among them.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use miri_runtime_core::block_cache::BLOCK_ALIGN;
use miri_runtime_core::{miri_rt_block_alloc, miri_rt_block_free};

const RECORD_CAPACITY: usize = 4096;

/// Addresses of blocks released at [`BLOCK_ALIGN`], in release order.
static RELEASED: [AtomicUsize; RECORD_CAPACITY] = [const { AtomicUsize::new(0) }; RECORD_CAPACITY];
static RELEASED_COUNT: AtomicUsize = AtomicUsize::new(0);

struct RecordingAllocator;

unsafe impl GlobalAlloc for RecordingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.align() == BLOCK_ALIGN {
            let slot = RELEASED_COUNT.fetch_add(1, Ordering::Relaxed);
            if let Some(record) = RELEASED.get(slot) {
                record.store(ptr as usize, Ordering::Relaxed);
            }
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: RecordingAllocator = RecordingAllocator;

fn released_addresses() -> Vec<usize> {
    let count = RELEASED_COUNT.load(Ordering::Relaxed).min(RECORD_CAPACITY);
    RELEASED[..count]
        .iter()
        .map(|record| record.load(Ordering::Relaxed))
        .collect()
}

#[test]
fn blocks_parked_by_a_thread_are_released_when_it_exits() {
    let parked: Vec<usize> = std::thread::spawn(|| {
        let blocks: Vec<*mut u8> = (0..64)
            .map(|_| unsafe { miri_rt_block_alloc(48) })
            .collect();
        for &block in &blocks {
            unsafe { miri_rt_block_free(block) };
        }
        blocks.into_iter().map(|block| block as usize).collect()
    })
    .join()
    .expect("the thread runs");
    let released = released_addresses();
    let missing: Vec<&usize> = parked
        .iter()
        .filter(|block| !released.contains(block))
        .collect();
    assert!(
        missing.is_empty(),
        "{} parked blocks outlived their thread",
        missing.len()
    );
}
