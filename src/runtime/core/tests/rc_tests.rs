// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::rc::*;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Serializes the tests that read a before/after delta of the process-wide
/// closure balance. The test harness runs them on parallel threads, so without
/// this one test's increment lands inside another's window and corrupts its
/// delta.
static BALANCE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Takes [`BALANCE_LOCK`] with tracking on: the runtime counts nothing once it
/// has settled that nothing observes the program, which is what its load-time
/// hook decides for a test process run with no check asked for.
fn lock_balance() -> MutexGuard<'static, ()> {
    miri_runtime_core::guard::miri_rt_tracking_state
        .store(miri_runtime_core::guard::TRACKING_ON, Ordering::Relaxed);
    BALANCE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn closure_alloc_track_increments_balance() {
    let _serialized = lock_balance();
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
    let _serialized = lock_balance();
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
    let _serialized = lock_balance();
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

/// Clones `original` once a second holder has made it shared, and checks the
/// clone is held by nothing else.
unsafe fn assert_clone_is_unshared<T>(
    kind: &str,
    original: *mut T,
    clone: unsafe extern "C" fn(*const T) -> *mut T,
    free: unsafe extern "C" fn(*mut T),
) {
    incref(original as *mut u8);
    assert!(
        miri_rt_rc_is_shared(original as *const u8),
        "{kind} original"
    );
    let copy = clone(original);
    assert!(!miri_rt_rc_is_shared(copy as *const u8), "{kind} clone");
    free(original);
    free(copy);
}

/// The compiler takes a value a collection clone hands back as held by
/// nothing else, and skips asking whether it is shared before writing
/// through it (`returns_unshared` in the compiler's runtime function table).
/// Each clone must therefore return a fresh block whose count is 1, even when
/// the collection cloned is shared.
#[test]
fn collection_clones_hand_back_blocks_nothing_else_holds() {
    use miri_runtime_core::array::ffi::{
        miri_rt_array_clone, miri_rt_array_free, miri_rt_array_new,
    };
    use miri_runtime_core::list::ffi::{miri_rt_list_clone, miri_rt_list_free, miri_rt_list_new};
    use miri_runtime_core::map::ffi::{miri_rt_map_clone, miri_rt_map_free, miri_rt_map_new};
    use miri_runtime_core::set::ffi::{miri_rt_set_clone, miri_rt_set_free, miri_rt_set_new};
    unsafe {
        let list = miri_rt_list_new(8);
        assert_clone_is_unshared("list", list, miri_rt_list_clone, miri_rt_list_free);
        let map = miri_rt_map_new(8, 8, 0);
        assert_clone_is_unshared("map", map, miri_rt_map_clone, miri_rt_map_free);
        let set = miri_rt_set_new(8);
        assert_clone_is_unshared("set", set, miri_rt_set_clone, miri_rt_set_free);
        let array = miri_rt_array_new(3, 8);
        assert_clone_is_unshared("array", array, miri_rt_array_clone, miri_rt_array_free);
    }
}
