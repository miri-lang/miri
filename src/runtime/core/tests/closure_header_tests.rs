// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closure header compiled code builds and the runtime reads: the
//! function word, then the environment's drop thunk, then the captures.

use miri_runtime_core::abi_layout::{
    ClosureHeader, CLOSURE_CAPTURES, CLOSURE_DROP, CLOSURE_FUNCTION,
};
use miri_runtime_core::block_cache::ffi::{miri_rt_block_alloc, miri_rt_block_free};
use miri_runtime_core::guard::INLINE_PAYLOAD_OFFSET;
use miri_runtime_core::rc::RC_HEADER_SIZE;
use miri_runtime_core::release::{miri_rt_release_defer, miri_rt_release_drain};
use std::mem::{offset_of, size_of};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const WORD: usize = size_of::<usize>();

#[test]
fn the_header_words_are_where_their_indices_say() {
    assert_eq!(offset_of!(ClosureHeader, function), CLOSURE_FUNCTION * WORD);
    assert_eq!(offset_of!(ClosureHeader, drop), CLOSURE_DROP * WORD);
    assert_eq!(size_of::<ClosureHeader>(), CLOSURE_CAPTURES * WORD);
}

/// Held by every test that defers a release: the release worklist belongs to
/// the one thread running compiled code, and the test harness runs tests on
/// several.
static WORKLIST_USE: Mutex<()> = Mutex::new(());

/// The payload the drop thunk below was handed, and the capture it read.
static DROPPED_PAYLOAD: AtomicUsize = AtomicUsize::new(0);
static DROPPED_CAPTURE: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn record_drop(payload: *mut u8) {
    DROPS.fetch_add(1, Ordering::SeqCst);
    DROPPED_PAYLOAD.store(payload as usize, Ordering::SeqCst);
    // SAFETY: the payload is the test's three-word closure below.
    let capture = unsafe { payload.cast::<usize>().add(CLOSURE_CAPTURES).read() };
    DROPPED_CAPTURE.store(capture, Ordering::SeqCst);
}

#[test]
fn a_deferred_closure_is_dropped_once_through_its_drop_word() {
    let _worklist = WORKLIST_USE
        .lock()
        .expect("the worklist lock is not poisoned");
    let mut payload: [usize; 3] = [0; 3];
    payload[CLOSURE_DROP] = record_drop as *const () as usize;
    payload[CLOSURE_CAPTURES] = 42;
    let payload_ptr = payload.as_mut_ptr().cast::<u8>();

    // SAFETY: the drop word holds a function of the drop-thunk signature,
    // which compiled code defers a closure under once its count is zero.
    unsafe {
        let drop_word = payload_ptr.cast::<usize>().add(CLOSURE_DROP).read();
        let thunk: unsafe extern "C" fn(*mut u8) = std::mem::transmute(drop_word);
        miri_rt_release_defer(payload_ptr, thunk);
        miri_rt_release_drain();
    }

    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(DROPPED_PAYLOAD.load(Ordering::SeqCst), payload_ptr as usize);
    assert_eq!(DROPPED_CAPTURE.load(Ordering::SeqCst), 42);
}

/// The blocks the chain's drop thunks below freed, in the order they ran.
static FREED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// A closure block as compiled code allocates it: `[size][RC = 1]`, then the
/// header naming `drop` and one capture word holding `capture`. Returns its
/// payload.
unsafe fn closure_block(drop: unsafe extern "C" fn(*mut u8), capture: usize) -> *mut u8 {
    let payload_bytes = (CLOSURE_CAPTURES + 1) * WORD;
    // SAFETY: the block is fresh and spans the header, the capture and the
    // two words in front of the payload.
    unsafe {
        let base = miri_rt_block_alloc(INLINE_PAYLOAD_OFFSET + payload_bytes);
        assert!(!base.is_null(), "the test block is allocated");
        let payload = base.add(INLINE_PAYLOAD_OFFSET);
        payload.sub(RC_HEADER_SIZE).cast::<usize>().write(1);
        let words = payload.cast::<usize>();
        words.add(CLOSURE_FUNCTION).write(0);
        words.add(CLOSURE_DROP).write(drop as *const () as usize);
        words.add(CLOSURE_CAPTURES).write(capture);
        payload
    }
}

/// Free the closure block whose payload is `payload`, recording it.
unsafe fn free_closure(payload: *mut u8) {
    FREED
        .lock()
        .expect("the record is not poisoned")
        .push(payload as usize);
    // SAFETY: `payload` is a block `closure_block` allocated, freed once.
    unsafe { miri_rt_block_free(payload.sub(INLINE_PAYLOAD_OFFSET)) };
}

/// The innermost closure's drop thunk: it captures nothing counted.
unsafe extern "C" fn drop_inner(payload: *mut u8) {
    // SAFETY: the drain hands back the payload `drop_outer` deferred.
    unsafe { free_closure(payload) };
}

/// The outer closure's drop thunk: its capture is the inner closure, whose
/// count it releases. Running inside the drain, it defers the inner closure
/// through that closure's own drop word rather than nesting a frame.
unsafe extern "C" fn drop_outer(payload: *mut u8) {
    // SAFETY: the payload is a block `closure_block` built, whose capture is
    // the inner closure's payload.
    unsafe {
        let inner = payload.cast::<usize>().add(CLOSURE_CAPTURES).read() as *mut u8;
        let count = inner.sub(RC_HEADER_SIZE).cast::<usize>();
        count.write(count.read() - 1);
        if count.read() == 0 {
            let drop_word = inner.cast::<usize>().add(CLOSURE_DROP).read();
            let thunk: unsafe extern "C" fn(*mut u8) = std::mem::transmute(drop_word);
            miri_rt_release_defer(inner, thunk);
        }
        free_closure(payload);
    }
}

#[test]
fn a_deferred_chain_of_two_closures_runs_both_thunks_and_frees_both_blocks() {
    let _worklist = WORKLIST_USE
        .lock()
        .expect("the worklist lock is not poisoned");
    // SAFETY: each block is a closure of the layout compiled code builds, its
    // count already zero when it is deferred, as the drain requires.
    let (outer, inner) = unsafe {
        let inner = closure_block(drop_inner, 0);
        let outer = closure_block(drop_outer, inner as usize);
        miri_rt_release_defer(outer, drop_outer);
        miri_rt_release_drain();
        (outer, inner)
    };

    let freed = FREED.lock().expect("the record is not poisoned").clone();
    assert_eq!(freed, vec![outer as usize, inner as usize]);
}
