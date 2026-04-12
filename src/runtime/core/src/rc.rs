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

/// Global counter: incremented on alloc, decremented on free.
/// A non-zero value at exit indicates a memory leak (positive) or double-free (negative).
static RC_ALLOC_BALANCE: AtomicIsize = AtomicIsize::new(0);

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
    let balance = RC_ALLOC_BALANCE.load(Ordering::SeqCst);
    if balance != 0 {
        // Use a raw write to stderr to avoid Rust's buffered I/O flushing issues.
        let msg = format!("MIRI_LEAK_CHECK: leaked {balance} RC allocation(s)\n");
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
