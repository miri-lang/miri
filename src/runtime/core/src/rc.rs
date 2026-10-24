// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference counting header utilities for heap-allocated Miri values.
//!
//! All heap-allocated types (strings, arrays, lists, user classes) share
//! the same memory layout: `[RC][payload]`. The variable holds a pointer
//! to the payload; the RC is at `ptr - RC_HEADER_SIZE`.
//!
//! This module provides helpers for allocation and deallocation with
//! this layout, so every heap type uses the same convention.
//!
//! When the `MIRI_LEAK_CHECK` environment variable is set to `1`, a global
//! allocation counter tracks alloc/free pairs and reports leaks at exit.

use std::sync::atomic::{AtomicIsize, Ordering};

use crate::guard::{guard_alloc, guard_free, report_and_abort, AllocKind, FreeVerdict};

/// Size of the reference count header, in bytes.
/// Matches `ptr_type.bytes()` in the Cranelift codegen.
pub const RC_HEADER_SIZE: usize = std::mem::size_of::<usize>();

/// Global counter for RC-tracked heap objects (strings, lists, arrays, classes, …).
/// Incremented on alloc, decremented on free. Non-zero at exit → leak or double-free.
static RC_ALLOC_BALANCE: AtomicIsize = AtomicIsize::new(0);

/// Global counter for closure heap allocations.
///
/// Closures are allocated by compiled code (not `alloc_with_rc`) because their
/// layout has an extra size header word. This separate counter lets the leak-check
/// atexit handler catch closure-only leaks that `RC_ALLOC_BALANCE` would miss.
pub static CLOSURE_ALLOC_BALANCE: AtomicIsize = AtomicIsize::new(0);

/// Nothing to do where the runtime registers its exit handlers from a
/// load-time initialiser (see `alloc_count`): they are in place before compiled
/// code first allocates, so allocation pays nothing for them.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[inline(always)]
fn ensure_exit_handler_registered() {}

/// Registers an `atexit` handler for exit reporting on first allocation, on a
/// platform with no load-time initialiser to do it.
/// Called once on first RC allocation, closure allocation, or inline allocation.
/// Ensures both alloc-count and leak-check handlers are registered as needed.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn ensure_exit_handler_registered() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Ensure alloc-count handler is registered if enabled.
        crate::handler_config::ensure_alloc_count_handler_registered();

        // Ensure leak-check handler is registered if enabled.
        crate::handler_config::ensure_leak_check_handler_registered();
    });
}

/// Whether allocations and releases are counted: always, except once the
/// runtime has settled that nothing observes them. The state moves away from
/// unsettled once, so a block counted on allocation is counted on release
/// whenever a leak check can read the balance.
#[inline]
fn is_counted() -> bool {
    crate::guard::miri_rt_tracking_state.load(Ordering::Relaxed) != crate::guard::TRACKING_OFF
}

/// Called at process exit to report any leaked allocations.
/// This is the leak-check portion of the exit handler; allocation count reporting
/// is handled separately by `alloc_count::report_at_exit()`.
///
/// The handler may have been registered at load time, before a release build's
/// entry point said its compiled code counts nothing; such a program's balance
/// covers only the runtime's half of the heap, so it is never reported.
pub(crate) fn report_leak_check_at_exit() {
    if crate::handler_config::checks_compiled_out() {
        return;
    }
    // Report allocation count first
    crate::alloc_count::report_at_exit();

    // Then check for leaks
    let rc_balance = RC_ALLOC_BALANCE.load(Ordering::Relaxed);
    let cl_balance = CLOSURE_ALLOC_BALANCE.load(Ordering::Relaxed);
    if rc_balance != 0 || cl_balance != 0 {
        // Use a raw write to stderr to avoid Rust's buffered I/O flushing issues.
        let msg = if rc_balance != 0 && cl_balance != 0 {
            format!(
                "MIRI_LEAK_CHECK: leaked {rc_balance} RC allocation(s) and {cl_balance} closure allocation(s)\n"
            )
        } else if rc_balance != 0 {
            format!("MIRI_LEAK_CHECK: leaked {rc_balance} RC allocation(s)\n")
        } else {
            format!("MIRI_LEAK_CHECK: leaked {cl_balance} closure allocation(s)\n")
        };
        // `_exit` below skips the profiler's atexit handler too; a leak
        // leaves the heap sound enough to write its record.
        crate::profile::finish();
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
            // Use _exit to bypass atexit handlers — calling std::process::exit here
            // would re-invoke atexit handlers recursively, causing undefined behaviour.
            libc::_exit(99);
        }
    }
}

/// Allocates `[RC=1][payload]` and returns a pointer to the payload.
///
/// The payload's bytes are unspecified — a small block is often one a
/// released value left behind — so the caller writes every byte it later
/// reads.
///
/// `#[track_caller]` so the heap guard records the *calling intrinsic's*
/// `file:line` (e.g. `list.rs:403`) rather than this function's own location.
/// Without it every allocation in the program shares one site and the guard's
/// leak report cannot attribute anything.
///
/// # Safety
/// The caller must ensure that `payload_size` together with the reference count
/// header fits in a valid memory layout.
#[track_caller]
pub unsafe fn alloc_with_rc(payload_size: usize) -> *mut u8 {
    ensure_exit_handler_registered();

    // Security Invariant: Use checked_add to prevent integer overflow when computing total layout size
    let Some(total_size) = RC_HEADER_SIZE.checked_add(payload_size) else {
        return std::ptr::null_mut();
    };
    let base = crate::block_cache::alloc_block(total_size);
    if base.is_null() {
        return std::ptr::null_mut();
    }

    // Set RC = 1
    *(base as *mut usize) = 1;

    if is_counted() {
        RC_ALLOC_BALANCE.fetch_add(1, Ordering::Relaxed);
        crate::alloc_count::increment_rc_count();
    }

    let payload_ptr = base.add(RC_HEADER_SIZE);

    // The kind is inferred by the guard from the tracked caller's source file,
    // since this shared entry point serves every collection and string. Passing
    // an explicit kind here would require a parameter at all six call sites.
    guard_alloc(payload_ptr, payload_size, AllocKind::Unknown);

    payload_ptr
}

/// Increments the RC of a managed heap object.
///
/// `ptr` must point to the payload (past the RC header). Immortal objects
/// (RC stored as a negative `isize`) are skipped silently.
///
/// # Safety
/// `ptr` must have been allocated via `alloc_with_rc`.
pub unsafe fn incref(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let rc_ptr = (ptr as usize - RC_HEADER_SIZE) as *mut usize;
    let rc = *rc_ptr;
    if (rc as isize) >= 0 {
        *rc_ptr = rc.saturating_add(1);
    }
}

/// Whether a managed heap object is reachable through more than one reference.
///
/// Compiled code asks this before writing into a value that has value
/// semantics — a struct, or a collection reached through a field — so a write
/// through one name is never seen through another: a shared value is copied
/// first. An immortal object (RC stored as a negative `isize`) answers `true`,
/// because every reader shares it for the whole run and nothing may write
/// through it. A null pointer is shared by no one.
///
/// # Safety
/// `ptr` must be null or point to the payload of a block whose RC word sits
/// immediately before it.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_rc_is_shared(ptr: *const u8) -> bool {
    // Compiled code answers this inline once the tracking state is off, and
    // calls here otherwise; settling the state keeps a platform whose
    // load-time hook did not run from calling here for good.
    crate::guard::resolve_tracking_state();
    if ptr.is_null() {
        return false;
    }
    crate::guard::guard_check(ptr as *mut u8);
    let rc = *((ptr as usize - RC_HEADER_SIZE) as *const usize) as isize;
    rc != 1
}

/// Stops the program unless the receiver a writing method was handed is held
/// by that call alone.
///
/// A method that writes its collection receiver is called on a collection its
/// caller made the method's own first, so its count is 1. The method checks
/// this where it would otherwise copy the receiver on write: a copy would
/// release a reference the method never held, and a write in place would
/// land in a value another name still holds. A count other than 1, an
/// immortal one included, is a caller that skipped that step, reported as
/// `MER_RT_017` in every build. Nothing is released or written. A null
/// pointer is held by no one.
///
/// # Safety
/// `ptr` must be null or point to the payload of a block whose RC word sits
/// immediately before it.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_receiver_unique(ptr: *const u8) {
    if ptr.is_null() {
        return;
    }
    crate::guard::guard_check(ptr as *mut u8);
    let rc = *((ptr as usize - RC_HEADER_SIZE) as *const usize) as isize;
    if rc != 1 {
        crate::trap::trap(
            crate::trap::code::RECEIVER_SHARED,
            "a method that writes its receiver was handed one something else still holds",
        );
    }
}

/// Frees the `[RC][payload]` block given a pointer to the payload.
///
/// `#[track_caller]` for the same reason as [`alloc_with_rc`]: the guard records
/// the freeing intrinsic's `file:line`, which is what names the two free sites in
/// a double-free report.
///
/// # Safety
/// `payload_ptr` must have been allocated via `alloc_with_rc` and `payload_size`
/// must be the same as was used during allocation.
#[track_caller]
pub unsafe fn free_with_rc(payload_ptr: *mut u8, payload_size: usize) {
    if payload_ptr.is_null() {
        return;
    }

    // Check with the heap guard (if enabled). The fatal verdicts report and
    // diverge, so a double-freed block is never released a second time.
    let verdict = guard_free(payload_ptr);

    if matches!(
        verdict,
        FreeVerdict::DoubleFree | FreeVerdict::WriteAfterFree
    ) {
        report_and_abort(verdict, payload_ptr as usize);
    }

    if is_counted() {
        RC_ALLOC_BALANCE.fetch_sub(1, Ordering::Relaxed);
    }

    // Only deallocate if the guard didn't quarantine the block.
    if verdict == FreeVerdict::Quarantine {
        // Block is quarantined; the guard will deallocate it later.
        return;
    }

    // Guard is disabled, returned DeallocNow, or the block was untracked.
    // Release it now.
    let base = payload_ptr.sub(RC_HEADER_SIZE);
    // Security Invariant: Use checked_add to prevent integer overflow when re-constructing total layout size
    let Some(total_size) = RC_HEADER_SIZE.checked_add(payload_size) else {
        return;
    };
    crate::block_cache::release_block(base, total_size);
}

/// Records that a closure heap allocation has been made.
///
/// Called by compiled Miri code immediately after `miri_rt_block_alloc`
/// allocates a closure struct. Registers the `atexit` leak-check handler on the first call.
///
/// # Safety
/// Must be matched by exactly one call to `miri_rt_closure_free_track` when the
/// closure is freed.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_closure_alloc_track() {
    ensure_exit_handler_registered();
    if is_counted() {
        CLOSURE_ALLOC_BALANCE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Records that a closure heap allocation has been freed.
///
/// Called by compiled Miri code immediately before `libc::free` releases a
/// closure struct whose RC has reached zero.
///
/// # Safety
/// Must be called exactly once per matching `miri_rt_closure_alloc_track` call.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_closure_free_track() {
    if is_counted() {
        CLOSURE_ALLOC_BALANCE.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Simulates a closure memory leak for testing the MIRI_LEAK_CHECK detector.
///
/// Increments `CLOSURE_ALLOC_BALANCE` by one without allocating a closure,
/// causing the atexit leak-check handler to report a spurious leak. Use this
/// from `system.testing` to write E2E tests that verify the detector fires.
///
/// # Safety
/// This function is for testing only. It intentionally unbalances the leak
/// counter; calling it in production code will produce a false leak report.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_closure_leak() {
    ensure_exit_handler_registered();
    CLOSURE_ALLOC_BALANCE.fetch_add(1, Ordering::Relaxed);
}

/// Records a block compiled code allocated itself with the heap guard.
///
/// Class instances, tuples, `Option`s, enum payloads and closure environments
/// are allocated inline by codegen rather than through [`alloc_with_rc`], so
/// they never reach the guard's shadow table on their own. Codegen emits a call
/// to this right after `miri_rt_block_alloc`, mirroring the existing
/// `miri_rt_closure_alloc_track` pattern.
///
/// `ptr` is the allocation base, which is also what `miri_rt_block_free` later
/// receives — keeping both sides of the pair keyed identically.
///
/// # Safety
/// `ptr` must be the pointer just returned by `miri_rt_block_alloc`, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_class_alloc_track(ptr: *mut u8) {
    crate::guard::resolve_tracking_state();
    ensure_exit_handler_registered();
    // Codegen emits this before its own null check, so a failed allocation arrives
    // here as null and must be ignored rather than recorded.
    if !ptr.is_null() {
        crate::alloc_count::increment_inline_count();
    }
    crate::guard::guard_alloc_raw(ptr, AllocKind::Class);
}

/// Records that compiled code is about to release a block it allocated itself.
///
/// The counterpart to [`miri_rt_class_alloc_track`]. Codegen frees the memory
/// itself, so unlike the runtime's own release path this block cannot be
/// quarantined; the guard still detects a double free and attributes the leak.
///
/// # Safety
/// `ptr` must be the allocation base about to be passed to `miri_rt_block_free`,
/// or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_class_free_track(ptr: *mut u8) {
    crate::guard::resolve_tracking_state();
    crate::guard::guard_free_raw(ptr);
}

/// Witnesses that compiled code is about to release a managed value, so the
/// heap guard can report a release of a block that was already freed before
/// the release reads that block's reference count.
///
/// `ptr` is the value's payload pointer, whichever layout it was allocated
/// with; nothing behind it is read.
#[no_mangle]
pub extern "C" fn miri_rt_release_check(ptr: *mut u8) {
    crate::guard::resolve_tracking_state();
    crate::rc_stats::count_drop();
    crate::guard::guard_check_release(ptr);
}

/// Frees a closure outright, as a spurious extra release would, so a test can
/// prove the heap guard reports the release compiled code makes afterwards.
///
/// The closure's own capture destructor is not run: this stands in for the
/// defect, not for a correct drop.
///
/// # Safety
/// For testing only. `closure` must be a live closure payload compiled code
/// allocated, or null. Without the heap guard the later release is a genuine
/// use after free.
///
/// Like every runtime parameter of a function type, the closure arrives with
/// the address of the trampoline it would be called through, which this one
/// never calls.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_closure_over_release(
    closure: *mut u8,
    _trampoline: *const u8,
) {
    if closure.is_null() {
        return;
    }
    miri_rt_closure_free_track();
    miri_rt_test_simulate_inline_over_release(closure);
}

/// Frees a block compiled code allocated inline — a class instance, a tuple,
/// an option — outright, as a spurious extra release would, so a test can
/// prove the heap guard reports the release compiled code makes afterwards.
///
/// Nothing the block holds is released: this stands in for the defect, not for
/// a correct drop. The block's layout is `[size][RC][payload]`, so its
/// base sits [`crate::guard::INLINE_PAYLOAD_OFFSET`] below `payload`.
///
/// # Safety
/// For testing only. `payload` must be the payload of a live block compiled
/// code allocated, or null. Without the heap guard the later release is a
/// genuine use after free.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_inline_over_release(payload: *mut u8) {
    if payload.is_null() {
        return;
    }
    let base = payload.sub(crate::guard::INLINE_PAYLOAD_OFFSET);
    miri_rt_class_free_track(base);
    crate::block_cache::ffi::miri_rt_block_free(base);
}

/// Frees the same allocation twice, to verify the heap guard's double-free trap.
///
/// With `MIRI_HEAP_GUARD=1` the second release is caught and the process aborts
/// naming the allocation site and both free sites. With the guard off this is a
/// genuine double free, so it exists purely so a test can prove the trap fires;
/// the counter-based leak check cannot see this class of bug at all, which is
/// the reason the guard exists.
///
/// # Safety
/// This function is for testing only. Without the guard enabled it corrupts the
/// heap by design; never call it from production code.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_double_free() {
    let payload_size = 32;
    let ptr = alloc_with_rc(payload_size);
    if ptr.is_null() {
        return;
    }
    free_with_rc(ptr, payload_size);
    free_with_rc(ptr, payload_size);
}

/// Hands a writing method's uniqueness check a receiver two references hold,
/// to verify it stops the program with `MER_RT_017` instead of going on.
///
/// The block is never released: the check ends the process first.
///
/// # Safety
/// This function is for testing only; it never returns.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_shared_receiver() {
    let ptr = alloc_with_rc(32);
    if ptr.is_null() {
        return;
    }
    incref(ptr);
    miri_rt_receiver_unique(ptr);
}

/// Serializes every test that allocates or frees through [`alloc_with_rc`] /
/// [`free_with_rc`]. Both maintain the process-global `RC_ALLOC_BALANCE`, so a
/// test asserting an exact balance delta races with any sibling test that
/// allocates concurrently. Holding this lock makes the delta deterministic
/// under the default multi-threaded test runner.
///
/// It lives at module scope rather than inside the test module because the
/// tests that perturb the counter are not all in this file: any test anywhere
/// in the crate that reaches these two functions has to take the same lock, or
/// the delta a balance assertion reads includes that test's allocations.
#[cfg(test)]
pub(crate) static BALANCE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires [`BALANCE_LOCK`], recovering the guard when an earlier panicking
/// test poisoned it: one test failing must not cascade into the others.
#[cfg(test)]
///
/// It also turns tracking on for the test process, so the balance a test reads
/// is counted: the load-time hook settles it to off when no check is asked for.
pub(crate) fn balance_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::guard::miri_rt_tracking_state.store(crate::guard::TRACKING_ON, Ordering::Relaxed);
    BALANCE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_with_rc_returns_pointer_with_rc_one() {
        let _balance = balance_guard();
        unsafe {
            let ptr = alloc_with_rc(64);
            assert!(!ptr.is_null(), "alloc_with_rc should not return null");

            // RC should be 1 at allocation.
            let rc_ptr = (ptr as usize - RC_HEADER_SIZE) as *mut usize;
            let rc = *rc_ptr;
            assert_eq!(rc, 1, "RC should be 1 after allocation");

            free_with_rc(ptr, 64);
        }
    }

    #[test]
    fn incref_increments_rc() {
        let _balance = balance_guard();
        unsafe {
            let ptr = alloc_with_rc(64);
            let rc_ptr = (ptr as usize - RC_HEADER_SIZE) as *mut usize;

            assert_eq!(*rc_ptr, 1, "initial RC should be 1");
            incref(ptr);
            assert_eq!(*rc_ptr, 2, "RC should be 2 after incref");
            incref(ptr);
            assert_eq!(*rc_ptr, 3, "RC should be 3 after second incref");

            free_with_rc(ptr, 64);
        }
    }

    #[test]
    fn incref_on_null_is_noop() {
        // incref on null pointer should not panic or crash.
        unsafe {
            incref(std::ptr::null_mut());
        }
    }

    #[test]
    fn incref_skips_immortal_objects() {
        let _balance = balance_guard();
        unsafe {
            let ptr = alloc_with_rc(64);
            let rc_ptr = (ptr as usize - RC_HEADER_SIZE) as *mut usize;

            // Mark as immortal by setting RC to negative isize.
            *rc_ptr = (-1isize) as usize;
            incref(ptr);
            // RC should remain unchanged (immortal objects are skipped).
            assert_eq!(*rc_ptr, (-1isize) as usize, "immortal RC should not change");

            free_with_rc(ptr, 64);
        }
    }

    #[test]
    fn test_alloc_and_free_with_rc_overflow_handled_safely() {
        let _balance = balance_guard();
        unsafe {
            let ptr = alloc_with_rc(usize::MAX);
            assert!(
                ptr.is_null(),
                "alloc_with_rc should return null on integer overflow"
            );

            let ptr_near_overflow = alloc_with_rc(usize::MAX - RC_HEADER_SIZE + 1);
            assert!(
                ptr_near_overflow.is_null(),
                "alloc_with_rc should return null on integer overflow"
            );

            free_with_rc(std::ptr::null_mut(), usize::MAX);
        }
    }

    #[test]
    fn free_with_rc_on_null_is_noop() {
        // free_with_rc on null pointer should not panic.
        unsafe {
            free_with_rc(std::ptr::null_mut(), 64);
        }
    }

    #[test]
    fn alloc_and_free_balance() {
        let _balance = balance_guard();
        unsafe {
            let before = RC_ALLOC_BALANCE.load(Ordering::Relaxed);

            let ptr1 = alloc_with_rc(64);
            let ptr2 = alloc_with_rc(128);
            let after_alloc = RC_ALLOC_BALANCE.load(Ordering::Relaxed);
            assert_eq!(after_alloc, before + 2, "balance should increase by 2");

            free_with_rc(ptr1, 64);
            free_with_rc(ptr2, 128);
            let after_free = RC_ALLOC_BALANCE.load(Ordering::Relaxed);
            assert_eq!(after_free, before, "balance should return to original");
        }
    }
}
