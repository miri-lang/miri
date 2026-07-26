// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::time::ffi::miri_rt_nanotime;
use std::time::{Duration, Instant};

#[test]
fn test_nanotime_is_not_negative() {
    // The value is handed to Miri code as a signed `int`; a negative reading
    // would make every elapsed-span subtraction meaningless.
    assert!(miri_rt_nanotime() >= 0);
}

#[test]
fn test_nanotime_never_goes_backwards() {
    let first = miri_rt_nanotime();
    let second = miri_rt_nanotime();
    assert!(
        second >= first,
        "nanotime went backwards: {first} then {second}"
    );
}

#[test]
fn test_nanotime_advances_over_a_sleep() {
    let sleep = Duration::from_millis(5);
    let before = miri_rt_nanotime();
    std::thread::sleep(sleep);
    let after = miri_rt_nanotime();

    let elapsed_ns = after - before;
    assert!(
        elapsed_ns >= sleep.as_nanos() as i64,
        "sleeping {sleep:?} advanced nanotime by only {elapsed_ns}ns"
    );
}

#[test]
fn test_nanotime_measures_the_same_span_as_instant() {
    // The clock is elapsed-since-start, so differences between two readings
    // must track wall-clock durations rather than an absolute epoch.
    let wall_start = Instant::now();
    let start = miri_rt_nanotime();
    std::thread::sleep(Duration::from_millis(10));
    let measured_ns = miri_rt_nanotime() - start;
    let wall_ns = wall_start.elapsed().as_nanos() as i64;

    assert!(measured_ns > 0, "no time passed: {measured_ns}ns");
    assert!(
        (measured_ns - wall_ns).abs() < Duration::from_millis(50).as_nanos() as i64,
        "nanotime span {measured_ns}ns disagrees with wall clock {wall_ns}ns"
    );
}
