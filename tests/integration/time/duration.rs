// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_duration_from_nanos() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_nanos(1000)
    println(f"{d.as_nanos()}")
"#,
        "1000",
    );
}

#[test]
fn test_duration_from_micros() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_micros(1000)
    println(f"{d.as_micros()}")
"#,
        "1000",
    );
}

#[test]
fn test_duration_from_millis() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_millis(1500)
    println(f"{d.as_millis()}")
"#,
        "1500",
    );
}

#[test]
fn test_duration_from_seconds() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_seconds(2)
    println(f"{d.as_seconds()}")
"#,
        "2",
    );
}

#[test]
fn test_duration_round_trip_millis() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let original = 1500
    let d = Duration.from_millis(original)
    let retrieved = d.as_millis()
    println(f"{retrieved == original}")
"#,
        "true",
    );
}

#[test]
fn test_duration_round_trip_seconds_to_millis() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_seconds(1)
    println(f"{d.as_millis()}")
"#,
        "1000",
    );
}

#[test]
fn test_duration_round_trip_millis_to_nanos() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_millis(1)
    println(f"{d.as_nanos()}")
"#,
        "1000000",
    );
}

#[test]
fn test_duration_zero() {
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_nanos(0)
    println(f"{d.as_nanos()}")
"#,
        "0",
    );
}

#[test]
fn test_duration_overflow_wrapping() {
    // Overflow behavior: large input multiplication wraps in two's complement.
    // from_seconds(9223372036854775) * 1_000_000_000 overflows and wraps.
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_seconds(9223372036854775)
    println(f"{d.as_nanos()}")
"#,
        "-808000000",
    );
}

#[test]
fn test_duration_negative_value_truncate_toward_zero() {
    // Negative nanos truncate toward zero when divided.
    // -1000 nanos / 1000 = -1 micros (truncates toward zero, not floor division)
    assert_runs_with_output(
        r#"
use system.time

fn main()
    let d = Duration.from_nanos(-1000)
    println(f"{d.as_micros()}")
"#,
        "-1",
    );
}
