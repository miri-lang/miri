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
static CLOSURE_ALLOC_BALANCE: AtomicIsize = AtomicIsize::new(0);

/// Registers an `atexit` handler that checks the allocation balance.
/// Called once on first allocation. Prints a diagnostic to stderr if
/// any RC-tracked allocations were not freed.
fn ensure_leak_check_registered() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Only register the atexit handler when MIRI_LEAK_CHECK=1
        if std::env::var("MIRI_LEAK_CHECK").as_deref() == Ok("1") {
            unsafe {
                libc::atexit(leak_check_at_exit);
            }
        }
    });
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
/// # Safety
/// The caller must ensure that `payload_size` together with the reference count
/// header fits in a valid memory layout.
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

    base.add(RC_HEADER_SIZE)
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
        *rc_ptr = rc + 1;
    }
}

/// Frees the `[RC][payload]` block given a pointer to the payload.
///
/// # Safety
/// `payload_ptr` must have been allocated via `alloc_with_rc` and `payload_size`
/// must be the same as was used during allocation.
pub unsafe fn free_with_rc(payload_ptr: *mut u8, payload_size: usize) {
    if payload_ptr.is_null() {
        return;
    }

    let base = payload_ptr.sub(RC_HEADER_SIZE);
    let total_size = RC_HEADER_SIZE + payload_size;
    let layout = Layout::from_size_align(total_size, 8).unwrap_or_else(|_| std::process::abort());
    dealloc(base, layout);

    RC_ALLOC_BALANCE.fetch_sub(1, Ordering::SeqCst);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
