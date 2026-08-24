// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_clock_now_creates_instant() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let clock = Clock()
    let instant = clock.now()
    println("instant created")
"#,
        "instant created",
    );
}

#[test]
fn test_instant_elapsed_is_positive() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let clock = Clock()
    let start = clock.now()
    let end = clock.now()
    let d = start.elapsed(clock)
    println(f"{d.as_nanos() >= 0}")
"#,
        "true",
    );
}

#[test]
fn test_sleep_short_duration() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let clock = Clock()
    let start = clock.now()
    clock.sleep(Duration.from_millis(10))
    let end = clock.now()
    let d = start.elapsed(clock)
    println(f"{d.as_millis() >= 10}")
"#,
        "true",
    );
}

#[test]
fn test_sleep_zero_duration() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let clock = Clock()
    clock.sleep(Duration.from_nanos(0))
    println("sleep zero returned")
"#,
        "sleep zero returned",
    );
}

#[test]
fn test_nanotime_deprecated() {
    assert_compiler_warning(
        r#"
use system.time

fn main()
    let t = nanotime()
    println("nanotime works")
"#,
        "MER_TYP_027",
    );
}

#[test]
fn test_nanotime_still_works() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let start = nanotime()
    let end = nanotime()
    println(f"{end >= start}")
"#,
        "true",
    );
}

#[test]
fn test_duration_nanos_field_is_private() {
    assert_compiler_error(
        r#"
use system.time

fn main()
    let d = Duration.from_millis(1000)
    println(f"{d.nanos}")
"#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_duration_nanos_field_not_writable() {
    assert_compiler_error(
        r#"
use system.time

fn main()
    let d = Duration.from_millis(1000)
    d.nanos = 42
"#,
        "Private and cannot be accessed",
    );
}

#[test]
fn test_instant_nanos_field_is_private() {
    assert_compiler_error(
        r#"
use system.time

fn main()
    let clock = Clock()
    let instant = clock.now()
    println(f"{instant.nanos}")
"#,
        "Private and cannot be accessed",
    );
}
