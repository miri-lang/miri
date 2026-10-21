// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Many threads making the process's first small allocations at once must
//! agree on how a parked block is marked: a thread that marked its blocks one
//! way and checked them another would take its own released block for one
//! written after release, and trap.
//!
//! The binary holds this one test, so these are the process's first blocks.

use std::sync::{Arc, Barrier};

use miri_runtime_core::{miri_rt_block_alloc, miri_rt_block_free};

#[test]
fn threads_settling_the_cache_at_once_agree_on_parked_blocks() {
    let threads = 32;
    let start = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..2000 {
                    unsafe {
                        let block = miri_rt_block_alloc(48);
                        miri_rt_block_free(block);
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        assert!(handle.join().is_ok());
    }
}
