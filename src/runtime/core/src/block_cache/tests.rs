// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The cache is per thread and the harness runs each test on a thread of its
//! own, so a test sees only the blocks it parked. What a process decides once —
//! the guard bypass, the poisoned mode — and what ends the process — a trap —
//! is tested in a child run of this same binary.

use super::ffi::{miri_rt_block_alloc, miri_rt_block_free};
use super::*;
use crate::rc::{alloc_with_rc, free_with_rc, RC_HEADER_SIZE};

/// Marks the tests a parent runs in a child process, so a plain run of the
/// suite passes over them.
const CHILD_VARIABLE: &str = "MIRI_BLOCK_CACHE_CHILD";

/// Runs the ignored test `name` in a child of this test binary with `env`
/// set, returning its exit status and stderr.
fn run_child(name: &str, env: &[(&str, &str)]) -> (std::process::ExitStatus, String) {
    let Ok(exe) = std::env::current_exe() else {
        panic!("the test binary has no path");
    };
    let mut command = std::process::Command::new(exe);
    command
        .args([
            &format!("block_cache::tests::{name}"),
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_VARIABLE, "1")
        .env_remove("MIRI_HEAP_GUARD")
        .env_remove("MIRI_LEAK_CHECK")
        .env_remove("MIRI_ALLOC_COUNT")
        .env_remove("MIRI_RC_STATS")
        .env_remove("MallocScribble");
    for (key, value) in env {
        command.env(key, value);
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => panic!("the child did not run: {error}"),
    };
    (
        output.status,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Whether this test is running as a parent's child.
fn is_child() -> bool {
    std::env::var_os(CHILD_VARIABLE).is_some()
}

fn assert_traps_with(name: &str, env: &[(&str, &str)], message: &str) {
    let (status, stderr) = run_child(name, env);
    assert_eq!(status.code(), Some(1), "the child did not trap:\n{stderr}");
    assert!(
        stderr.contains(message),
        "expected `{message}` in:\n{stderr}"
    );
}

#[test]
fn block_of_sixteen_bytes_and_one_of_seventeen_take_neighbouring_classes() {
    unsafe {
        let small = alloc_block(16);
        let large = alloc_block(17);
        let (before_small, before_large) = (parked_count(16), parked_count(32));
        release_block(small, 16);
        release_block(large, 17);
        assert_eq!(parked_count(16), before_small + 1);
        assert_eq!(parked_count(32), before_large + 1);
    }
}

#[test]
fn block_of_the_largest_class_is_parked_and_one_byte_more_is_not() {
    unsafe {
        let largest = alloc_block(LARGEST_CACHED_BLOCK);
        let past = alloc_block(LARGEST_CACHED_BLOCK + 1);
        let before = parked_bytes();
        release_block(largest, LARGEST_CACHED_BLOCK);
        assert_eq!(parked_bytes(), before + LARGEST_CACHED_BLOCK);
        release_block(past, LARGEST_CACHED_BLOCK + 1);
        assert_eq!(parked_bytes(), before + LARGEST_CACHED_BLOCK);
    }
}

#[test]
fn rc_payload_of_eight_bytes_fills_the_first_class_and_nine_the_second() {
    assert_eq!(RC_HEADER_SIZE, 8);
    unsafe {
        let eight = alloc_with_rc(8);
        let nine = alloc_with_rc(9);
        let (first, second) = (parked_count(16), parked_count(32));
        free_with_rc(eight, 8);
        free_with_rc(nine, 9);
        assert_eq!(parked_count(16), first + 1);
        assert_eq!(parked_count(32), second + 1);
    }
}

#[test]
fn every_byte_of_a_class_is_the_block_s_own() {
    unsafe {
        let block = alloc_block(17);
        assert!(system_block_size(block).is_some_and(|size| size >= 32));
        block.write_bytes(0x5A, 32);
        release_block(block, 17);
        let again = alloc_block(32);
        assert_eq!(block, again, "a request of the class missed the block");
        again.write_bytes(0x5A, 32);
        release_block(again, 32);
    }
}

#[test]
fn every_class_is_sixteen_byte_sized_and_aligned() {
    for class in 0..CLASS_COUNT {
        let size = class_size(class);
        assert_eq!(size % BLOCK_ALIGN, 0, "class {class} is {size} bytes");
        let block = unsafe { alloc_block(size) };
        assert_eq!(
            block as usize % BLOCK_ALIGN,
            0,
            "class {class} block {block:p}"
        );
        unsafe { release_block(block, size) };
    }
}

#[test]
fn a_thread_parks_every_block_it_held_live_even_past_the_floor() {
    let count = PARKED_BYTES_FLOOR / LARGEST_CACHED_BLOCK + 100;
    let blocks: Vec<*mut u8> = (0..count)
        .map(|_| unsafe { alloc_block(LARGEST_CACHED_BLOCK) })
        .collect();
    let spare = parked_bytes();
    for &block in &blocks {
        unsafe { release_block(block, LARGEST_CACHED_BLOCK) };
    }
    assert_eq!(parked_bytes(), spare + count * LARGEST_CACHED_BLOCK);
    let taken = unsafe { alloc_block(LARGEST_CACHED_BLOCK) };
    assert!(blocks.contains(&taken), "a parked block was not handed out");
    unsafe { release_block(taken, LARGEST_CACHED_BLOCK) };
}

#[test]
fn blocks_released_past_what_the_thread_held_live_go_back_to_the_system() {
    // Blocks another thread allocated count towards nothing this thread held,
    // so this thread parks them only up to the floor.
    let count = PARKED_BYTES_FLOOR / LARGEST_CACHED_BLOCK + 1;
    let addresses: Vec<usize> = match std::thread::spawn(move || {
        (0..count)
            .map(|_| unsafe { alloc_block(LARGEST_CACHED_BLOCK) } as usize)
            .collect()
    })
    .join()
    {
        Ok(addresses) => addresses,
        Err(_) => panic!("the allocating thread failed"),
    };
    let start = parked_count(LARGEST_CACHED_BLOCK);
    let spare = parked_bytes();
    for &address in &addresses {
        unsafe { release_block(address as *mut u8, LARGEST_CACHED_BLOCK) };
    }
    assert_eq!(parked_bytes(), PARKED_BYTES_FLOOR);
    let parked = parked_count(LARGEST_CACHED_BLOCK) - start;
    assert_eq!(parked, (PARKED_BYTES_FLOOR - spare) / LARGEST_CACHED_BLOCK);
}

#[test]
fn inline_block_of_fewer_than_eight_bytes_records_eight() {
    for size in 0..8 {
        unsafe {
            let block = miri_rt_block_alloc(size);
            assert!(!block.is_null());
            assert_eq!(block.cast::<usize>().read(), 8, "for a request of {size}");
            miri_rt_block_free(block);
        }
    }
}

#[test]
fn releasing_a_null_inline_block_does_nothing() {
    let before = parked_bytes();
    unsafe { miri_rt_block_free(std::ptr::null_mut()) };
    assert_eq!(parked_bytes(), before);
}

#[test]
fn heap_guard_bypasses_the_cache() {
    let (status, stderr) = run_child("child_parks_nothing", &[("MIRI_HEAP_GUARD", "1")]);
    assert!(status.success(), "{stderr}");
}

#[test]
#[ignore = "run by heap_guard_bypasses_the_cache in a child process"]
fn child_parks_nothing() {
    assert!(is_child());
    unsafe {
        let block = miri_rt_block_alloc(48);
        miri_rt_block_free(block);
    }
    assert_eq!(parked_count(48), 0);
    assert_eq!(cache_mode(), CacheMode::Bypassed);
}

#[test]
fn block_released_twice_traps() {
    assert_traps_with(
        "child_releases_twice",
        &[],
        "a heap block was released twice",
    );
}

#[test]
#[ignore = "run by block_released_twice_traps in a child process"]
fn child_releases_twice() {
    assert!(is_child());
    unsafe {
        let block = alloc_block(48);
        release_block(block, 48);
        release_block(block, 48);
    }
}

#[test]
fn block_written_while_parked_traps_when_handed_out() {
    assert_traps_with(
        "child_writes_a_parked_block",
        &[],
        "a heap block was written after it was released",
    );
}

#[test]
#[ignore = "run by block_written_while_parked_traps_when_handed_out in a child process"]
fn child_writes_a_parked_block() {
    assert!(is_child());
    unsafe {
        let block = alloc_block(48);
        release_block(block, 48);
        block.cast::<usize>().add(1).write(2);
        alloc_block(48);
    }
}

#[test]
fn scribble_poisons_released_and_handed_out_blocks() {
    let (status, stderr) = run_child("child_sees_poison", &[("MallocScribble", "1")]);
    assert!(status.success(), "{stderr}");
}

#[test]
#[ignore = "run by scribble_poisons_released_and_handed_out_blocks in a child process"]
fn child_sees_poison() {
    assert!(is_child());
    unsafe {
        let fresh = alloc_block(48);
        assert_eq!(cache_mode(), CacheMode::Poisoned);
        let fresh_bytes = std::slice::from_raw_parts(fresh.add(PARKED_HEADER), 32);
        assert!(fresh_bytes.iter().all(|&byte| byte == HANDED_OUT_FILL));
        release_block(fresh, 48);
        let parked = std::slice::from_raw_parts(fresh.add(PARKED_HEADER), 32);
        assert!(parked.iter().all(|&byte| byte == RELEASED_FILL));
        let held: Vec<*mut u8> = (0..QUARANTINE_DEPTH).map(|_| alloc_block(48)).collect();
        assert!(
            !held.contains(&fresh),
            "a block was handed out inside its quarantine"
        );
        for &block in &held {
            release_block(block, 48);
        }
        let reused = alloc_block(48);
        assert_eq!(reused, fresh, "the oldest released block comes back first");
        let bytes = std::slice::from_raw_parts(reused, 48);
        assert!(bytes.iter().all(|&byte| byte == HANDED_OUT_FILL));
    }
}

#[test]
fn scribble_traps_a_write_into_a_released_block() {
    assert_traps_with(
        "child_writes_into_poison",
        &[("MallocScribble", "1")],
        "a heap block was written after it was released",
    );
}

#[test]
#[ignore = "run by scribble_traps_a_write_into_a_released_block in a child process"]
fn child_writes_into_poison() {
    assert!(is_child());
    unsafe {
        let first = alloc_block(48);
        release_block(first, 48);
        first.add(40).write(7);
        for _ in 0..=QUARANTINE_DEPTH {
            let block = alloc_block(48);
            release_block(block, 48);
        }
    }
}

#[test]
fn scribble_traps_a_release_larger_than_the_block() {
    assert_traps_with(
        "child_releases_too_large",
        &[("MallocScribble", "1")],
        "released at a size larger than it was allocated at",
    );
}

#[test]
#[ignore = "run by scribble_traps_a_release_larger_than_the_block in a child process"]
fn child_releases_too_large() {
    assert!(is_child());
    unsafe {
        let block = alloc_block(16);
        release_block(block, LARGEST_CACHED_BLOCK);
    }
}

#[test]
fn blocks_parked_on_one_thread_are_invisible_to_a_concurrent_one() {
    let (status, stderr) = run_child("child_parks_beside_a_second_thread", &[]);
    assert!(status.success(), "{stderr}");
}

#[test]
#[ignore = "run by blocks_parked_on_one_thread_are_invisible_to_a_concurrent_one in a child process"]
fn child_parks_beside_a_second_thread() {
    assert!(is_child());
    unsafe {
        let first = alloc_block(48);
        release_block(first, 48);
    }
    assert_eq!(parked_count(48), 1);
    let second = std::thread::spawn(|| {
        let seen = parked_count(48);
        let block = unsafe { alloc_block(48) };
        unsafe { release_block(block, 48) };
        (seen, parked_count(48))
    });
    let Ok((seen, parked_after)) = second.join() else {
        panic!("the second thread panicked");
    };
    assert_eq!(seen, 0, "the second thread saw the first thread's block");
    assert_eq!(parked_after, 1, "the second thread did not park its block");
    assert_eq!(parked_count(48), 1, "the second thread took a parked block");
}

#[test]
fn thread_exit_leaves_its_cache_to_no_later_thread() {
    let (status, stderr) = run_child("child_outlives_the_first_thread", &[]);
    assert!(status.success(), "{stderr}");
}

/// The first thread to reach the cache parks a block and exits; a thread
/// started after it, which on Apple arm64 may be handed the same thread
/// storage, must find an empty cache that still parks.
#[test]
#[ignore = "run by thread_exit_leaves_its_cache_to_no_later_thread in a child process"]
fn child_outlives_the_first_thread() {
    assert!(is_child());
    let park_one = || {
        let seen = parked_count(48);
        unsafe {
            let block = alloc_block(48);
            release_block(block, 48);
        }
        (seen, parked_count(48))
    };
    let Ok(first) = std::thread::spawn(park_one).join() else {
        panic!("the first thread panicked");
    };
    assert_eq!(first, (0, 1));
    #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
    assert!(is_owner_retired(), "the exiting owner kept its cache");
    let Ok(later) = std::thread::spawn(park_one).join() else {
        panic!("the later thread panicked");
    };
    assert_eq!(later, (0, 1), "the later thread inherited a cache");
}
