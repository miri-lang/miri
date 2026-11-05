// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A named argument binds the parameter it names wherever it is written, on
//! every kind of callee, and arguments are still evaluated in the order they
//! are written.

use super::utils::*;

#[test]
fn a_static_function_binds_named_arguments_by_name() {
    assert_runs_with_output(
        r#"
class C
    static fn s(x int, y int) int
        return x * 10 + y

fn main()
    println(f"{C.s(y: 2, x: 1)} {C.s(1, y: 2)} {C.s(1, 2)}")
"#,
        "12 12 12",
    );
}

#[test]
fn an_enum_static_function_binds_named_arguments_by_name() {
    assert_runs_with_output(
        r#"
enum Dir
    Up
    Down

    static fn pick(up bool, n int) int
        if up
            return n
        return 0 - n

fn main()
    println(f"{Dir.pick(n: 3, up: true)} {Dir.pick(n: 3, up: false)}")
"#,
        "3 -3",
    );
}

#[test]
fn a_math_intrinsic_binds_named_arguments_by_name() {
    assert_runs_with_output(
        r#"
use system.math

fn main()
    println(f"{pow(exp: 3.0, base: 2.0)} {pow(2.0, exp: 3.0)}")
"#,
        "8.0 8.0",
    );
}

#[test]
fn named_arguments_to_a_static_function_are_evaluated_as_written() {
    assert_heap_guard_output(
        r#"
class C
    static fn s(x String, y String) String
        return x + "|" + y

fn say(t String) String
    println(t)
    return t

fn main()
    println(C.s(y: say("first"), x: say("second")))
"#,
        "first\nsecond\nsecond|first",
    );
}

#[test]
fn a_math_intrinsic_through_a_module_alias_binds_named_arguments_by_name() {
    assert_runs_with_output(
        r#"
use system.math as M

fn main()
    println(f"{M.pow(exp: 3.0, base: 2.0)} {M.mix(t: 0.5, a: 0.0, b: 4.0)}")
"#,
        "8.0 2.0",
    );
}

/// `min` and `max` hold their arguments at the one number type the checker
/// settled on, however the intrinsic is named.
#[test]
fn min_and_max_through_a_module_alias_combine_mixed_widths() {
    assert_runs_with_output(
        r#"
use system.math as M

fn main()
    let a i32 = 1
    let b int = 5
    println(f"{M.max(a, b)} {M.min(b, a)}")
"#,
        "5 1",
    );
}
