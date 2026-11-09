// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A program whose recursion outgrows the stack stops with a runtime error, not
//! a signal.

use super::utils::*;

const STACK_OVERFLOW: &str = "stack overflow";

#[test]
fn test_unbounded_recursion_is_reported_as_a_stack_overflow() {
    assert_runtime_error(
        r#"
use system.io

fn inf(n int) int
    return inf(n + 1) + 1

fn main()
    println("start")
    println(f"{inf(0)}")
"#,
        STACK_OVERFLOW,
    );
}

#[test]
fn test_mutual_recursion_past_the_stack_is_reported_as_a_stack_overflow() {
    assert_runtime_error(
        r#"
use system.io

fn ev(n int) bool
    if n == 0
        return true
    return od(n - 1) and true

fn od(n int) bool
    if n == 0
        return false
    return ev(n - 1) and true

fn main()
    println(f"{ev(100000000)}")
"#,
        STACK_OVERFLOW,
    );
}

/// The recursion a program is allowed: deep, inside the stack it has.
#[test]
fn test_deep_recursion_inside_the_stack_returns_its_value() {
    assert_runs_with_output(
        r#"
use system.io

fn down(n int) int
    if n == 0
        return 0
    return 1 + down(n - 1)

fn main()
    println(f"{down(100000)}")
"#,
        "100000",
    );
}

/// A fault elsewhere is not the stack running out: the program keeps ending
/// by the signal, and no overflow is claimed for it.
#[test]
fn test_a_fault_away_from_the_stack_keeps_its_signal() {
    let result = crate::utils::miri_run(
        r#"
runtime "core" fn miri_rt_test_simulate_wild_read()

fn main()
    miri_rt_test_simulate_wild_read()
"#,
    );
    let output = result.output();
    assert!(!result.success, "the wild read did not fault:\n{output}");
    assert!(
        !output.contains(STACK_OVERFLOW),
        "a wild read was reported as a stack overflow:\n{output}"
    );
    assert!(
        output.contains("terminated by signal"),
        "the wild read did not end by its signal:\n{output}"
    );
}
