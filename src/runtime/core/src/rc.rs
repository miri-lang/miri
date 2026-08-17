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

use std::alloc::{alloc_zeroed, dealloc, Layout};
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
/// Closures use `libc::malloc` directly (not `alloc_with_rc`) because their layout
/// has an extra `malloc_ptr` header word. This separate counter lets the leak-check
/// atexit handler catch closure-only leaks that `RC_ALLOC_BALANCE` would miss.
pub static CLOSURE_ALLOC_BALANCE: AtomicIsize = AtomicIsize::new(0);

/// Registers an `atexit` handler that checks the allocation balance.
/// Called once on first allocation. Prints a diagnostic to stderr if
/// any RC-tracked allocations were not freed.
fn ensure_leak_check_registered() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if is_leak_check_enabled() {
            unsafe {
                libc::atexit(leak_check_at_exit);
            }
        }
    });
}

/// Whether the allocation-balance check was asked for.
///
/// Read directly rather than inferred from whether the exit handler was
/// registered: the tracking state compiled code consults is settled on the
/// first allocation, which is the same moment registration happens, so it
/// cannot ask about the outcome of the thing it is deciding.
pub fn is_leak_check_enabled() -> bool {
    std::env::var("MIRI_LEAK_CHECK").as_deref() == Ok("1")
}

/// Called at process exit to report any leaked allocations.
extern "C" fn leak_check_at_exit() {
    let rc_balance = RC_ALLOC_BALANCE.load(Ordering::SeqCst);
    let cl_balance = CLOSURE_ALLOC_BALANCE.load(Ordering::SeqCst);
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
    ensure_leak_check_registered();

    let total_size = RC_HEADER_SIZE + payload_size;
    let layout = match Layout::from_size_align(total_size, 8) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };

    let base = alloc_zeroed(layout);
    if base.is_null() {
        return std::ptr::null_mut();
    }

    // Set RC = 1
    *(base as *mut usize) = 1;

    RC_ALLOC_BALANCE.fetch_add(1, Ordering::SeqCst);

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

    // Always decrement the balance counter.
    RC_ALLOC_BALANCE.fetch_sub(1, Ordering::SeqCst);

    // Only deallocate if the guard didn't quarantine the block.
    if verdict == FreeVerdict::Quarantine {
        // Block is quarantined; the guard will deallocate it later.
        return;
    }

    // Guard is disabled, returned DeallocNow, or the block was untracked.
    // Deallocate immediately.
    let base = payload_ptr.sub(RC_HEADER_SIZE);
    let total_size = RC_HEADER_SIZE + payload_size;
    let layout = Layout::from_size_align(total_size, 8).unwrap_or_else(|_| std::process::abort());
    dealloc(base, layout);
}

/// Records that a closure heap allocation has been made.
///
/// Called by compiled Miri code immediately after `libc::malloc` allocates a
/// closure struct. Registers the `atexit` leak-check handler on the first call.
///
/// # Safety
/// Must be matched by exactly one call to `miri_rt_closure_free_track` when the
/// closure is freed.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_closure_alloc_track() {
    ensure_leak_check_registered();
    CLOSURE_ALLOC_BALANCE.fetch_add(1, Ordering::SeqCst);
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
    CLOSURE_ALLOC_BALANCE.fetch_sub(1, Ordering::SeqCst);
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
    ensure_leak_check_registered();
    CLOSURE_ALLOC_BALANCE.fetch_add(1, Ordering::SeqCst);
}

/// Records a raw `libc::malloc` made by compiled code with the heap guard.
///
/// Class instances, tuples, `Option`s, enum payloads and closure environments
/// are allocated inline by codegen rather than through [`alloc_with_rc`], so
/// they never reach the guard's shadow table on their own. Codegen emits a call
/// to this right after the malloc, mirroring the existing
/// `miri_rt_closure_alloc_track` pattern.
///
/// `ptr` is the raw allocation base, which is also what `free` later receives —
/// keeping both sides of the pair keyed identically.
///
/// # Safety
/// `ptr` must be the pointer just returned by `malloc`, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_class_alloc_track(ptr: *mut u8) {
    crate::guard::resolve_tracking_state();
    ensure_leak_check_registered();
    // Codegen emits this before its own null check, so a failed malloc arrives
    // here as null and must be ignored rather than recorded.
    crate::guard::guard_alloc_raw(ptr, AllocKind::Class);
}

/// Records that compiled code is about to `libc::free` a raw allocation.
///
/// The counterpart to [`miri_rt_class_alloc_track`]. Codegen frees the memory
/// itself, so unlike the runtime's own release path this block cannot be
/// quarantined; the guard still detects a double free and attributes the leak.
///
/// # Safety
/// `ptr` must be the allocation base about to be passed to `free`, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_class_free_track(ptr: *mut u8) {
    crate::guard::resolve_tracking_state();
    crate::guard::guard_free_raw(ptr);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes every test that allocates or frees through `alloc_with_rc` /
    /// `free_with_rc`. Both maintain the process-global `RC_ALLOC_BALANCE`, so a
    /// test asserting an exact balance delta races with any sibling test that
    /// allocates concurrently. Holding this lock makes the delta deterministic
    /// under the default multi-threaded test runner.
    static BALANCE_LOCK: Mutex<()> = Mutex::new(());

    /// Acquires [`BALANCE_LOCK`], recovering the guard when an earlier panicking
    /// test poisoned it: one test failing must not cascade into the others.
    fn balance_guard() -> MutexGuard<'static, ()> {
        BALANCE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
            let before = RC_ALLOC_BALANCE.load(Ordering::SeqCst);

            let ptr1 = alloc_with_rc(64);
            let ptr2 = alloc_with_rc(128);
            let after_alloc = RC_ALLOC_BALANCE.load(Ordering::SeqCst);
            assert_eq!(after_alloc, before + 2, "balance should increase by 2");

            free_with_rc(ptr1, 64);
            free_with_rc(ptr2, 128);
            let after_free = RC_ALLOC_BALANCE.load(Ordering::SeqCst);
            assert_eq!(after_free, before, "balance should return to original");
        }
    }
}
