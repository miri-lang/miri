// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How much heap the runtime functions that build a large string hold at
//! their peak.
//!
//! This test binary installs a global allocator that tracks the bytes live
//! and the most ever live, so it can see every buffer a function makes: Rust's
//! own and the Miri string's, which come from the same global allocator. It is
//! its own binary because a global allocator is per binary, and its tests take
//! one lock so no other test's allocations land in a measured window.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use miri_runtime_core::fs::ffi::{miri_rt_fs_read_file, miri_rt_fs_status};
use miri_runtime_core::list::ffi::{miri_rt_list_new, miri_rt_list_push};
use miri_runtime_core::string::{miri_rt_string_free, miri_rt_string_join, MiriString};

struct PeakTracking;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for PeakTracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(live, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static ALLOCATOR: PeakTracking = PeakTracking;

static MEASURING: Mutex<()> = Mutex::new(());

/// The bytes `f` holds above what was live when it started, at its peak.
fn peak_above<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let result = f();
    (result, PEAK.load(Ordering::SeqCst) - before)
}

const FILE_BYTES: usize = 8 << 20;
const SLACK: usize = 1 << 20;

#[test]
fn test_reading_a_file_holds_its_bytes_once() {
    let _measuring = MEASURING.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("miri-fs-read-peak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("input.txt");
    let text: String = (0..FILE_BYTES)
        .map(|i| (b'a' + (i % 26) as u8) as char)
        .collect();
    std::fs::write(&file, &text).expect("write input");
    drop(text);
    let path = MiriString::from_str(file.to_str().expect("utf-8 temp path"));

    // SAFETY: `path` is a live string; the result is freed below.
    let (contents, peak) = peak_above(|| unsafe { miri_rt_fs_read_file(&path) });

    // SAFETY: `contents` is the live string `read_file` returned.
    unsafe {
        assert_eq!(miri_rt_fs_status(), 0);
        assert_eq!((*contents).len(), FILE_BYTES);
        assert!((*contents)
            .as_str()
            .starts_with("abcdefghijklmnopqrstuvwxyzabc"));
        assert!(
            (*contents).ascii,
            "an all-ASCII file reads as an ASCII string"
        );
        miri_rt_string_free(contents);
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        peak < FILE_BYTES + SLACK,
        "reading an {FILE_BYTES}-byte file held {peak} bytes at its peak"
    );
}

#[test]
fn test_joining_strings_holds_the_result_once() {
    let _measuring = MEASURING.lock().unwrap_or_else(|e| e.into_inner());
    const PARTS: usize = 64;
    const PART_BYTES: usize = 128 << 10;
    let part: String = "é".repeat(PART_BYTES / 2);
    let separator = MiriString::from_str(",");
    // SAFETY: the list holds string pointers, one word each, and every string
    // pushed is a live string the list is given.
    let (joined, peak) = unsafe {
        let parts = miri_rt_list_new(std::mem::size_of::<usize>());
        for _ in 0..PARTS {
            let string = Box::into_raw(Box::new(MiriString::from_str(&part)));
            miri_rt_list_push(
                parts,
                &string as *const *mut MiriString as *const u8,
                std::mem::size_of::<usize>(),
            );
        }
        peak_above(|| miri_rt_string_join(&separator, parts))
    };
    let expected = PARTS * PART_BYTES + PARTS - 1;
    // SAFETY: `joined` is the live string `join` returned.
    unsafe {
        assert_eq!((*joined).len(), expected);
        assert!((*joined).as_str().starts_with("éé"));
        assert!(!(*joined).ascii, "a join of non-ASCII parts is not ASCII");
        miri_rt_string_free(joined);
    }
    assert!(
        peak < expected + SLACK,
        "joining into a {expected}-byte string held {peak} bytes at its peak"
    );
}
