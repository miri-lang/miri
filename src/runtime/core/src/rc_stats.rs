// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-counting operation counter.
//!
//! Activated by the `MIRI_RC_STATS=1` environment variable. Counts what the
//! program's reference counting does, for the benchmark suite's diagnosis of
//! a memory loss:
//! - `dup`: increments compiled code made;
//! - `drop`: decrements compiled code made;
//! - `drop_free`: decrements that reached zero and released the value;
//! - `cow_copy`: copies a write made of a value it found shared — a
//!   collection copied by its copy-on-write entry point or by a write through
//!   a path, or a struct rebuilt by one;
//! - `realloc`: buffers the runtime grew in place or moved.
//!
//! Compiled code reaches the counters only through the hooks it guards with
//! the runtime's tracking state, so with every observer off an increment or a
//! decrement costs one load and one predicted branch, and nothing more. A
//! release build carries no hooks at all, and never counts. At exit
//! the counts are printed to stderr after the allocation count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

static DUP: AtomicUsize = AtomicUsize::new(0);
static DROP: AtomicUsize = AtomicUsize::new(0);
static DROP_FREE: AtomicUsize = AtomicUsize::new(0);
static COW_COPY: AtomicUsize = AtomicUsize::new(0);
static REALLOC: AtomicUsize = AtomicUsize::new(0);

/// Whether `MIRI_RC_STATS=1` was set, read once, in a program that carries
/// the checking hooks.
pub fn is_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    !crate::handler_config::checks_compiled_out()
        && *ENABLED.get_or_init(|| std::env::var("MIRI_RC_STATS").as_deref() == Ok("1"))
}

fn count(counter: &AtomicUsize) {
    if is_enabled() {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// Counts a decrement compiled code made; called from the release check every
/// decrement already reports to.
pub fn count_drop() {
    count(&DROP);
}

/// Counts a write that copied a value it found shared.
pub fn count_cow_copy() {
    count(&COW_COPY);
}

/// Counts a buffer the runtime grew or moved.
pub fn count_realloc() {
    count(&REALLOC);
}

/// Hook compiled code calls after a write through a path copied a value it
/// found shared — a collection cloned or a struct rebuilt — while the
/// runtime's tracking state is on.
#[no_mangle]
pub extern "C" fn miri_rt_rc_stats_cow_copy() {
    count_cow_copy();
}

/// Hook compiled code calls after incrementing a reference count, while the
/// runtime's tracking state is on.
#[no_mangle]
pub extern "C" fn miri_rt_rc_stats_dup() {
    count(&DUP);
}

/// Hook compiled code calls when a decrement reached zero, before it releases
/// the value, while the runtime's tracking state is on.
#[no_mangle]
pub extern "C" fn miri_rt_rc_stats_free() {
    count(&DROP_FREE);
}

/// The counts so far, in the order the exit report prints them.
pub fn snapshot() -> [usize; 5] {
    [&DUP, &DROP, &DROP_FREE, &COW_COPY, &REALLOC].map(|c| c.load(Ordering::Relaxed))
}

/// Prints the counts to stderr, when the counter is on.
pub fn report_at_exit() {
    if !is_enabled() {
        return;
    }
    let [dup, drop, drop_free, cow_copy, realloc] = snapshot();
    let msg = format!(
        "MIRI_RC_STATS: dup={dup} drop={drop} drop_free={drop_free} cow_copy={cow_copy} realloc={realloc}\n"
    );
    unsafe {
        libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
    }
}

extern "C" fn report_at_exit_wrapper() {
    report_at_exit();
}

/// Registers the exit report, once, when the counter is on.
pub fn ensure_handler_registered() {
    static INIT: OnceLock<()> = OnceLock::new();
    if is_enabled() {
        let _ = INIT.get_or_init(|| unsafe {
            libc::atexit(report_at_exit_wrapper);
        });
    }
}
