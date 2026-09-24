// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Acceptance criterion: integer capture.
#[test]
fn test_capture_int() {
    assert_runs_with_output(
        r#"

fn main()
    var x = 10
    let f = fn() int: x + 1
    println(f"{f()}")
    println(f"{f()}")
    "#,
        "11\n11",
    );
}

/// Capture and use in expression with lambda param.
#[test]
fn test_capture_with_param() {
    assert_runs_with_output(
        r#"

fn main()
    let base = 10
    let add = fn(n int) int: base + n
    println(f"{add(5)}")
    "#,
        "15",
    );
}

/// Capture multiple variables.
#[test]
fn test_capture_multiple() {
    assert_runs_with_output(
        r#"

fn main()
    let a = 3
    let b = 4
    let sum = fn() int: a + b
    println(f"{sum()}")
    "#,
        "7",
    );
}

/// Capture a string (pointer capture).
#[test]
fn test_capture_string() {
    assert_runs_with_output(
        r#"

fn main()
    let greeting = "Hello"
    let f = fn() String: greeting
    println(f())
    "#,
        "Hello",
    );
}

/// A local the body uses only as an index is captured.
#[test]
fn test_capture_used_only_as_list_index() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let xs = List([10, 20, 30])
    let i = 2
    let f = fn() int: xs[i]
    println(f"{f()}")
"#,
        "30",
    );
}

/// A local the body uses only as an array index is captured.
#[test]
fn test_capture_used_only_as_array_index() {
    assert_runs_with_output(
        r#"
fn main()
    let a = [7, 8, 9]
    let i = 1
    let h = fn() int: a[i]
    println(f"{h()}")
"#,
        "8",
    );
}

/// A class instance the body only stores into is captured. The capture copies
/// the reference, so the store reaches the enclosing scope's instance.
#[test]
fn test_capture_used_only_as_field_store_base() {
    assert_runs_with_output(
        r#"
class Box
    var v int

fn main()
    var b = Box(v: 1)
    let g = fn()
        b.v = 7
        return
    g()
    println(f"{b.v}")
"#,
        "7",
    );
}

/// A local the body uses only as the index of a store is captured. The list is
/// a copy, so the store stays inside the closure.
#[test]
fn test_capture_used_only_as_index_of_a_store() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List([1, 2, 3])
    let i = 2
    let g = fn()
        xs[i] = 42
        println(f"{xs[0]} {xs[2]}")
        return
    g()
    println(f"{xs[0]} {xs[1]} {xs[2]}")
"#,
        "1 42\n1 2 3",
    );
}
