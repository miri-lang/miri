// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Unit tests for time operations.

use miri_runtime_core::{miri_rt_nanotime, miri_rt_sleep_nanos};
use std::time::Instant;

#[test]
fn test_sleep_advances_clock() {
    let before = miri_rt_nanotime();
    let start = Instant::now();
    miri_rt_sleep_nanos(50_000_000); // 50 milliseconds
    let elapsed = start.elapsed();
    let after = miri_rt_nanotime();

    // Check that nanotime advanced
    assert!(after >= before, "nanotime should advance after sleep");

    // Check that actual elapsed time is at least the requested duration
    // (accounting for system jitter, we don't assert an upper bound)
    assert!(
        elapsed.as_nanos() as i64 >= 50_000_000,
        "elapsed time should be at least the sleep duration"
    );
}

#[test]
fn test_sleep_zero_returns_promptly() {
    let start = Instant::now();
    miri_rt_sleep_nanos(0);
    let elapsed = start.elapsed();

    // Sleep with zero should return immediately (within a few milliseconds)
    assert!(elapsed.as_millis() < 10, "sleep(0) should return promptly");
}

#[test]
fn test_sleep_negative_returns_promptly() {
    let start = Instant::now();
    miri_rt_sleep_nanos(-1000);
    let elapsed = start.elapsed();

    // Sleep with negative duration should return immediately
    assert!(
        elapsed.as_millis() < 10,
        "sleep(negative) should return promptly"
    );
}

#[test]
fn test_sleep_does_not_panic() {
    // This test simply verifies that sleep doesn't panic on edge cases
    miri_rt_sleep_nanos(0);
    miri_rt_sleep_nanos(-1);
    miri_rt_sleep_nanos(i64::MIN);
    miri_rt_sleep_nanos(1_000_000); // 1 millisecond
}
