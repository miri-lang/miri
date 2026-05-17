// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_assert_true_passes_silently() {
    assert_runs_with_output(
        r#"
use system.io
use system.testing

fn main()
    assert(1 + 1 == 2)
    println("done")
"#,
        "done",
    );
}

#[test]
fn test_assert_false_panics() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert(1 == 2)
"#,
        "assertion failed",
    );
}

#[test]
fn test_assert_false_includes_source_location() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert(false)
"#,
        ":5",
    );
}

#[test]
fn test_assert_with_message() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert(false, "balance must be non-negative")
"#,
        "balance must be non-negative",
    );
}

#[test]
fn test_assert_true_with_message_does_not_panic() {
    assert_runs_with_output(
        r#"
use system.io
use system.testing

fn main()
    assert(true, "should not appear")
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_inside_user_function_pass() {
    // Verifies the failure-path location plumbing and runtime call work from
    // a function with an explicit allocator parameter.
    assert_runs_with_output(
        r#"
use system.io
use system.testing

fn require(x int)
    assert(x > 0, "x must be positive")

fn main()
    require(5)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_inside_user_function_fail() {
    assert_runtime_error(
        r#"
use system.testing

fn require(x int)
    assert(x > 0, "x must be positive")

fn main()
    require(0)
"#,
        "x must be positive",
    );
}
