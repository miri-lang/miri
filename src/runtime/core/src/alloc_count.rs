// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Heap allocation counter for measuring total memory allocations.
//!
//! Activated by the `MIRI_ALLOC_COUNT=1` environment variable.
//! Maintains counters for each allocation category:
//! - RC: structures with reference counting (strings, lists, maps, sets, user classes)
//! - Inline: inline aggregates (tuples, Options, enum payloads, closures)
//! - Buffers: collection element storage (list/map/set/array buffers)
//! - Raw: raw allocations (string data, etc.)
//!
//! Counters are monotonic and never decremented. At exit, prints the total
//! allocation count and breakdown by kind to stderr.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for RC-tracked struct allocations (strings, lists, maps, sets, classes).
static RC_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Global counter for inline aggregate allocations (tuples, Options, enums, closures).
static INLINE_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Global counter for collection buffer allocations (element storage).
static BUFFER_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Global counter for raw memory allocations (string data, etc.).
static RAW_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Increments the RC allocation counter.
#[inline]
pub fn increment_rc_count() {
    RC_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Increments the inline aggregate allocation counter.
#[inline]
pub fn increment_inline_count() {
    INLINE_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Increments the collection buffer allocation counter.
#[inline]
pub fn increment_buffer_count() {
    BUFFER_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Increments the raw allocation counter.
#[inline]
pub fn increment_raw_count() {
    RAW_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Read the current RC allocation count (for testing).
#[inline]
pub fn get_rc_count() -> usize {
    RC_ALLOC_COUNT.load(Ordering::Relaxed)
}

/// Read the current inline allocation count (for testing).
#[inline]
pub fn get_inline_count() -> usize {
    INLINE_ALLOC_COUNT.load(Ordering::Relaxed)
}

/// Read the current buffer allocation count (for testing).
#[inline]
pub fn get_buffer_count() -> usize {
    BUFFER_ALLOC_COUNT.load(Ordering::Relaxed)
}

/// Read the current raw allocation count (for testing).
#[inline]
pub fn get_raw_count() -> usize {
    RAW_ALLOC_COUNT.load(Ordering::Relaxed)
}

/// Called at process exit to report allocation counts if enabled.
///
/// This is called by the unified exit handler that also handles leak checking.
/// The allocation count is printed first, followed by any leak report.
pub fn report_at_exit() {
    if !crate::handler_config::is_alloc_count_enabled() {
        return;
    }

    let rc = RC_ALLOC_COUNT.load(Ordering::Relaxed);
    let inline = INLINE_ALLOC_COUNT.load(Ordering::Relaxed);
    let buffers = BUFFER_ALLOC_COUNT.load(Ordering::Relaxed);
    let raw = RAW_ALLOC_COUNT.load(Ordering::Relaxed);
    let total = rc + inline + buffers + raw;

    let msg = format!(
        "MIRI_ALLOC_COUNT: {} allocation(s) (rc={} inline={} buffers={} raw={})\n",
        total, rc, inline, buffers, raw
    );

    unsafe {
        libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
    }
}

// Platform-specific linker sections to initialize handlers at library load time.
// This ensures even zero-allocation programs register an atexit handler to report
// their allocation count.

#[cfg(target_os = "macos")]
#[link_section = "__DATA,__mod_init_func"]
#[used]
static INIT_SECTION_MACOS: unsafe extern "C" fn() =
    crate::handler_config::init_handlers_at_load_time;

#[cfg(target_os = "linux")]
#[link_section = ".init_array"]
#[used]
static INIT_SECTION_LINUX: unsafe extern "C" fn() =
    crate::handler_config::init_handlers_at_load_time;

// On Windows, the initialization relies on the first allocation to trigger
// handler registration, so we do not use a linker section.
#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "linux"),
    not(target_os = "windows")
))]
compile_error!("Allocation counter initialization: platform not explicitly handled. Add support for this platform or ensure first allocation triggers initialization.");
