// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────────────────────────────────────
// Baseline: managed params inside function bodies must not trigger false-positive
// use-after-move errors (regression guard for Phase 12 escape analysis).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_managed_param_passed_to_readonly_fn_no_error() {
    // Passing a list to a function that only reads it must never be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn length_of(items [int]) int
    return items.length()

fn check(items [int])
    let n = length_of(items)
    let m = length_of(items)
    println(f"{n} {m}")

let xs = List([1, 2, 3])
check(xs)
"#,
    );
}

#[test]
fn test_managed_param_multi_pass_no_error() {
    // Passing the same managed param to multiple calls inside a function body
    // must not consume it.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn sum(items [int]) int
    var s = 0
    var i = 0
    while i < items.length()
        s = s + items.element_at(i)
        i = i + 1
    return s

fn double_sum(items [int]) int
    let a = sum(items)
    let b = sum(items)
    return a + b

let xs = List([1, 2, 3])
println(f"{double_sum(xs)}")
"#,
    );
}

#[test]
fn test_managed_param_passed_transitively_no_error() {
    // A managed param passed through a helper chain that never stores it
    // must not be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn inner(items [int]) int
    return items.length()

fn middle(items [int]) int
    return inner(items)

fn outer(items [int]) int
    return middle(items)

let xs = List([10, 20])
println(f"{outer(xs)}")
"#,
    );
}

#[test]
fn test_managed_param_recursive_no_error() {
    // A recursive function that passes the same managed param to itself
    // must not be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn count_down(items [int], n int) int
    if n <= 0
        return 0
    return items.element_at(0) + count_down(items, n - 1)

let xs = List([1, 2, 3])
println(f"{count_down(xs, 3)}")
"#,
    );
}

#[test]
fn test_resource_param_inside_fn_body_still_errors() {
    // Resource types (those with fn drop(self)) must still be flagged inside
    // function bodies — Phase 12 does not change resource semantics.
    assert_compiler_error(
        r#"
use system.io

struct Conn
    host String
    fn drop(self)
        println("closed")

fn sink(c Conn)
    println(c.host)

fn use_twice(c Conn)
    sink(c)
    sink(c)

let c = Conn("db.local")
use_twice(c)
"#,
        "consumed",
    );
}
