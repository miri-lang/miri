// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The worklist that keeps a release from recursing past a fixed depth.
//!
//! A class's drop thunk releases its members, and a member that reaches zero
//! is dropped from inside it, so an object chain of length n would otherwise
//! take n native frames to release. The chain's length is data the program
//! builds; the stack it has is not. Every drop thunk therefore counts its
//! nesting in [`miri_rt_release_depth`], and past the depth the compiler chose
//! hands the object here instead of dropping it.
//!
//! The invariant: every object whose count reached zero is dropped exactly
//! once, its own hook before its members, and before the outermost release
//! returns. A chain's hooks run in chain order; objects deferred together run
//! in the order they were deferred, so only members released after a deferred
//! sibling, past the depth limit, can see a different order than recursion
//! would give.
//!
//! Compiled Miri code is single-threaded, as its reference counts are; the
//! atomics exist so compiled code can read these as plain words beside the
//! runtime, as [`crate::guard::miri_rt_tracking_state`] is read.

use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many drop thunks are currently nested. Read and written by compiled
/// code directly.
#[no_mangle]
pub static miri_rt_release_depth: AtomicUsize = AtomicUsize::new(0);

/// How many objects are waiting to be dropped. Compiled code reads it to skip
/// the call to [`miri_rt_release_drain`] when there is nothing to do.
#[no_mangle]
pub static miri_rt_release_pending: AtomicUsize = AtomicUsize::new(0);

/// A drop thunk: the function that drops one object it receives by payload.
type DropThunk = unsafe extern "C" fn(*mut u8);

/// The objects waiting to be dropped, with the thunk that drops each.
struct Worklist(UnsafeCell<VecDeque<(*mut u8, DropThunk)>>);

// SAFETY: compiled Miri code is single-threaded, and only it reaches here.
unsafe impl Sync for Worklist {}

static WORKLIST: Worklist = Worklist(UnsafeCell::new(VecDeque::new()));

/// The next waiting object, recording what remains.
///
/// # Safety
/// Only the single thread running compiled Miri code may call this.
unsafe fn next_deferred() -> Option<(*mut u8, DropThunk)> {
    // SAFETY: the caller is on the one thread that reaches the worklist, and
    // the reference ends before any thunk runs.
    let worklist = unsafe { &mut *WORKLIST.0.get() };
    let next = worklist.pop_front();
    miri_rt_release_pending.store(worklist.len(), Ordering::Relaxed);
    next
}

/// Records `payload` to be dropped through `drop_thunk` once the current
/// release has unwound.
///
/// # Safety
/// `payload` must be an object whose count reached zero, and `drop_thunk` the
/// function that drops an object of its type.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_release_defer(payload: *mut u8, drop_thunk: DropThunk) {
    // SAFETY: compiled Miri code is single-threaded; nothing else reaches the
    // worklist, and no thunk runs while this reference exists.
    let worklist = unsafe { &mut *WORKLIST.0.get() };
    worklist.push_back((payload, drop_thunk));
    miri_rt_release_pending.store(worklist.len(), Ordering::Relaxed);
}

/// Drops every waiting object, and whatever they defer in turn.
///
/// Called by the outermost drop thunk, with the depth back at zero. The depth
/// stays above zero while the loop runs, so a thunk the loop calls does not
/// reach this function a second time.
///
/// # Safety
/// Only the outermost drop thunk may call this, on the thread that runs
/// compiled Miri code.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_release_drain() {
    miri_rt_release_depth.store(1, Ordering::Relaxed);
    // SAFETY: each entry is an object and its thunk, as `miri_rt_release_defer`
    // requires.
    while let Some((payload, drop_thunk)) = unsafe { next_deferred() } {
        unsafe { drop_thunk(payload) };
    }
    miri_rt_release_depth.store(0, Ordering::Relaxed);
}
