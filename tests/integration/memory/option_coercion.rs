// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference counting when a `T`-valued expression is coerced into a `T?`.
//!
//! A call result is a fresh value the callee already donated. Wrapping that
//! temp into an `Option` aggregate reads it, so the read must be balanced by a
//! release of the temp — otherwise the value outlives every holder.

use super::super::utils::*;

#[test]
fn test_return_call_result_coerced_to_option_is_balanced() {
    assert_heap_guard_ok(
        r#"
fn build() String
    return "a" + "b"

fn take() String?
    return build()

fn main()
    let v = take() ?? "none"
    println(v)
"#,
    );
}

#[test]
fn test_return_method_result_coerced_to_option_is_balanced() {
    assert_heap_guard_ok(
        r#"
use system.collections.list

fn take(l List<String>) String?
    return l.remove_at(0)

fn main()
    var l = List<String>()
    l.push("a" + "b")
    let v = take(l) ?? "none"
    println(v)
"#,
    );
}

#[test]
fn test_option_coerced_return_still_yields_the_value() {
    assert_runs_with_output(
        r#"
fn build() String
    return "a" + "b"

fn take() String?
    return build()

fn main()
    let v = take() ?? "none"
    println(v)
"#,
        "ab",
    );
}

#[test]
fn test_var_declared_option_from_call_result_is_balanced() {
    assert_heap_guard_ok(
        r#"
fn build() String
    return "a" + "b"

fn main()
    var x String? = build()
    let v = x ?? "none"
    println(v)
"#,
    );
}

#[test]
fn test_argument_coerced_to_option_is_balanced() {
    assert_heap_guard_ok(
        r#"
fn build() String
    return "a" + "b"

fn take(x String?) int
    let v = x ?? "none"
    return v.length()

fn main()
    println(f"{take(build())}")
"#,
    );
}

#[test]
fn test_assignment_coerced_to_option_is_balanced() {
    assert_heap_guard_ok(
        r#"
fn build() String
    return "a" + "b"

fn main()
    var x String? = None
    x = build()
    let v = x ?? "none"
    println(v)
"#,
    );
}
