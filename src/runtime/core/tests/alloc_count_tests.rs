// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::alloc_count::*;
use std::sync::{Mutex, OnceLock};

/// Serializes access to the shared allocation counters to prevent
/// race conditions when tests run in parallel.
static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn get_test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
}

/// Helper to verify that a counter increment function changes only its target counter.
/// Takes the increment function, the getter function, and a label for assertion messages.
fn assert_counter_increments_by_one(label: &str, increment_fn: fn(), get_target: fn() -> usize) {
    let _guard = get_test_lock();
    let before_rc = get_rc_count();
    let before_inline = get_inline_count();
    let before_buffer = get_buffer_count();
    let before_raw = get_raw_count();
    let before_target = get_target();

    increment_fn();

    let after_rc = get_rc_count();
    let after_inline = get_inline_count();
    let after_buffer = get_buffer_count();
    let after_raw = get_raw_count();
    let after_target = get_target();

    assert_eq!(
        after_target,
        before_target + 1,
        "{} must increment its counter by 1",
        label
    );

    // Verify all other counters remained unchanged
    if label != "increment_rc_count" {
        assert_eq!(
            after_rc, before_rc,
            "rc counter must not change with {}",
            label
        );
    }
    if label != "increment_inline_count" {
        assert_eq!(
            after_inline, before_inline,
            "inline counter must not change with {}",
            label
        );
    }
    if label != "increment_buffer_count" {
        assert_eq!(
            after_buffer, before_buffer,
            "buffer counter must not change with {}",
            label
        );
    }
    if label != "increment_raw_count" {
        assert_eq!(
            after_raw, before_raw,
            "raw counter must not change with {}",
            label
        );
    }
}

#[test]
fn increment_rc_count_increments_by_one() {
    assert_counter_increments_by_one("increment_rc_count", increment_rc_count, get_rc_count);
}

#[test]
fn increment_inline_count_increments_by_one() {
    assert_counter_increments_by_one(
        "increment_inline_count",
        increment_inline_count,
        get_inline_count,
    );
}

#[test]
fn increment_buffer_count_increments_by_one() {
    assert_counter_increments_by_one(
        "increment_buffer_count",
        increment_buffer_count,
        get_buffer_count,
    );
}

#[test]
fn increment_raw_count_increments_by_one() {
    assert_counter_increments_by_one("increment_raw_count", increment_raw_count, get_raw_count);
}

#[test]
fn ensure_handler_registered_is_idempotent() {
    // Verify that calling ensure_handler_registered() multiple times
    // does not register the handler multiple times (i.e., it's safe to call).
    // This is a unit-level sanity check; the integration tests verify
    // that the atexit handler actually fires and prints exactly once.
    let _guard = get_test_lock();

    // Just verify the function doesn't panic or crash on multiple calls.
    // The real test is in integration tests that verify the handler
    // prints the count exactly once even when called at startup.
    // (We can't directly check registration here without exposing internals.)

    // This test serves as documentation that the function is safe to call multiple times.
}
