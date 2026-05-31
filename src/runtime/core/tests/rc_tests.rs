// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::rc::*;
use std::sync::atomic::Ordering;

#[test]
fn closure_alloc_track_increments_balance() {
    let before = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    unsafe { miri_rt_closure_alloc_track() };
    let after = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    assert_eq!(
        after,
        before + 1,
        "alloc_track must increment CLOSURE_ALLOC_BALANCE"
    );
    // Restore balance so other tests are unaffected.
    unsafe { miri_rt_closure_free_track() };
}

#[test]
fn closure_free_track_decrements_balance() {
    let before = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    unsafe { miri_rt_closure_alloc_track() };
    unsafe { miri_rt_closure_free_track() };
    let after = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    assert_eq!(
        after, before,
        "balanced alloc+free must leave CLOSURE_ALLOC_BALANCE unchanged"
    );
}

#[test]
fn unmatched_alloc_leaves_nonzero_balance() {
    let before = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    unsafe { miri_rt_closure_alloc_track() };
    let mid = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
    assert_ne!(
        mid, before,
        "unmatched alloc_track must leave a non-zero residual"
    );
    // Restore.
    unsafe { miri_rt_closure_free_track() };
}
