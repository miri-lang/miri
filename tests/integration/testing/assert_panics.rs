// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_assert_panics_when_closure_panics() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_panics(fn(): panic("boom"))
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_panics_with_expected_substring_pass() {
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    assert_panics(fn(): panic("division by zero"), "division")
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_panics_when_closure_does_not_panic() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_panics(fn(): print(""))
"#,
        "closure did not panic",
    );
}

#[test]
fn test_assert_panics_expected_substring_mismatch() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    assert_panics(fn(): panic("real reason"), "different message")
"#,
        "expected panic containing",
    );
}

#[test]
fn test_assert_panics_with_capturing_closure() {
    // The closure captures `reason` from the enclosing scope. Verifies the
    // closure layout / env-pointer plumbing survives the sigsetjmp/longjmp
    // catch path.
    assert_runs_with_output(
        r#"
use system.testing

fn main()
    let reason = "capture-path failed"
    assert_panics(fn(): panic(reason), "capture-path")
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_assert_panics_inside_user_function() {
    // Verifies the catch frame and TLS PANIC_CATCH_BUF are usable from a
    // function with an injected allocator parameter (not just main).
    assert_runs_with_output(
        r#"
use system.testing

fn check_boom()
    assert_panics(fn(): panic("inside"))

fn main()
    check_boom()
    println("ok")
"#,
        "ok",
    );
}
