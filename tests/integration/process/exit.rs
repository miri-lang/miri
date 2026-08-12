// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_run;

#[test]
fn test_exit_zero_reports_success_and_flushes_output() {
    let result = miri_run(
        r#"
use system.process

fn main()
    println("before exit")
    exit(0)
"#,
    );
    assert!(
        result.success,
        "exit(0) should report success, got:\n{}",
        result.output()
    );
    assert!(
        result.output().contains("before exit"),
        "Output printed before exit(0) must not be lost:\n{}",
        result.output()
    );
}

#[test]
fn test_exit_nonzero_reports_failure_and_flushes_output() {
    let result = miri_run(
        r#"
use system.process

fn main()
    println("before exit")
    exit(3)
"#,
    );
    assert!(
        !result.success,
        "exit(3) should report failure, got:\n{}",
        result.output()
    );
    assert!(
        result.output().contains("before exit"),
        "Output printed before exit(3) must not be lost:\n{}",
        result.output()
    );
}

/// A shell observes only the low 8 bits of an exit code, so a code of 256 is
/// indistinguishable from 0. This pins the documented wrapping behavior.
#[test]
fn test_exit_code_wraps_to_low_eight_bits() {
    let result = miri_run(
        r#"
use system.process

fn main()
    println("before exit")
    exit(256)
"#,
    );
    assert!(
        result.success,
        "exit(256) wraps to 0 and should report success, got:\n{}",
        result.output()
    );
    assert!(
        result.output().contains("before exit"),
        "Output printed before exit(256) must not be lost:\n{}",
        result.output()
    );
}

/// Exiting while a managed value is still live must not be reported as a leak:
/// an early exit deliberately skips destructors.
#[test]
fn test_exit_with_live_managed_value_is_not_a_leak() {
    let result = miri_run(
        r#"
use system.process

fn main()
    let held = "a" + "b"
    println(held)
    exit(0)
"#,
    );
    assert!(
        result.success,
        "exit(0) holding a live String should report success, got:\n{}",
        result.output()
    );
    assert!(
        !result.stderr.contains("MIRI_LEAK_CHECK"),
        "An intentional early exit must not be reported as a leak:\n{}",
        result.output()
    );
}

#[test]
fn test_exit_prevents_subsequent_code() {
    let result = miri_run(
        r#"
use system.process

fn main()
    println("line 1")
    println("line 2")
    println("line 3")
    exit(0)
    println("never printed")
"#,
    );
    assert!(
        result.output().contains("line 1\nline 2\nline 3"),
        "All lines before exit() should be printed in order:\n{}",
        result.output()
    );
    assert!(
        !result.output().contains("never printed"),
        "Code after exit() should not execute:\n{}",
        result.output()
    );
}
