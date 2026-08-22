// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Unified handler registration and enablement predicates for allocation counting
//! and leak checking.
//!
//! This module consolidates the decision logic for when to register atexit handlers,
//! avoiding circular dependencies between `alloc_count.rs` and `rc.rs`.

use std::sync::OnceLock;

/// Whether the allocation counter was requested via MIRI_ALLOC_COUNT=1.
pub fn is_alloc_count_enabled() -> bool {
    std::env::var("MIRI_ALLOC_COUNT").as_deref() == Ok("1")
}

/// Whether the allocation-balance check was asked for.
pub fn is_leak_check_enabled() -> bool {
    std::env::var("MIRI_LEAK_CHECK").as_deref() == Ok("1")
}

/// Wrapper for atexit that calls the alloc-count report function.
extern "C" fn alloc_count_exit_handler_wrapper() {
    crate::alloc_count::report_at_exit();
}

/// Wrapper for atexit that calls the leak-check report function.
extern "C" fn leak_check_exit_handler_wrapper() {
    crate::rc::report_leak_check_at_exit();
}

/// Ensures the allocation counter exit handler is registered.
///
/// This is called in two places:
/// 1. At library load time via `init_at_load_time()` for zero-alloc support
/// 2. On first allocation from rc.rs to ensure it's registered early
///
/// The handler is only registered if `MIRI_ALLOC_COUNT` is on AND leak checking
/// is off. When both are on, the leak-check handler will print the count instead.
pub fn ensure_alloc_count_handler_registered() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Only register if alloc counting is on AND leak check is off.
        // If leak check is on, the leak-check handler will print the count.
        if is_alloc_count_enabled() && !is_leak_check_enabled() {
            unsafe {
                libc::atexit(alloc_count_exit_handler_wrapper);
            }
        }
    });
}

/// Ensures the leak-check handler is registered.
///
/// This is called from:
/// 1. This module during initialization (for zero-alloc support)
/// 2. The rc module on first allocation (normal path)
///
/// Both paths use the same `OnceLock` to prevent double registration.
static LEAK_HANDLER_INIT: OnceLock<()> = OnceLock::new();

/// Ensures the leak-check handler is registered exactly once.
/// Safe to call from any context; uses atomic synchronization.
pub fn ensure_leak_check_handler_registered() {
    if is_leak_check_enabled() {
        let _ = LEAK_HANDLER_INIT.get_or_init(|| unsafe {
            libc::atexit(leak_check_exit_handler_wrapper);
        });
    }
}

/// Initializes both handlers at library load time.
/// Called via platform-specific linker sections to ensure it runs before main(),
/// allowing zero-allocation programs to report their count.
///
/// # Safety
/// Safe to call from any context; called only by the linker at library initialization.
pub unsafe extern "C" fn init_handlers_at_load_time() {
    if is_alloc_count_enabled() {
        // Register alloc-count handler if enabled (and leak-check is off)
        ensure_alloc_count_handler_registered();
        // If leak-check is also on, register its handler at startup
        // so that zero-alloc programs report their count
        if is_leak_check_enabled() {
            ensure_leak_check_handler_registered();
        }
    } else if is_leak_check_enabled() {
        // If only leak-check is on (no alloc count), register the handler
        ensure_leak_check_handler_registered();
    }
}
