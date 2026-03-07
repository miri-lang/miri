// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

#[test]
fn test_tuple_creation() {
    assert_runs("let t = (1, 2, 3)");
}

#[test]
fn test_tuple_single_element() {
    assert_runs("let t = (42,)");
}

#[test]
fn test_tuple_mixed_types() {
    assert_runs(r#"let t = (1, "hello", true)"#);
}

#[test]
fn test_tuple_access() {
    assert_runs_with_output(
        r#"
use system.io

let t = (10, 20, 30)
print(f"{t.0 + t.1}")
    "#,
        "30",
    );
}

// ==================== Function Passing / Return ====================

#[test]
fn test_tuple_passed_to_function() {
    assert_runs_with_output(
        r#"
use system.io

fn print_tuple(t (int, String))
    println(f"{t.0} {t.1}")

fn main()
    let t = (42, "hello")
    print_tuple(t: t)
"#,
        "42 hello",
    );
}

#[test]
fn test_tuple_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io

fn make_tuple() (int, bool)
    return (99, true)

fn main()
    let t = make_tuple()
    println(f"{t.0} {t.1}")
"#,
        "99 true",
    );
}

// ==================== Destructuring ====================

#[test]
fn test_tuple_destructuring() {
    assert_runs_with_output(
        r#"
use system.io

let t = (10, 20)
let sum = match t
    (a, b): a + b
println(f"{sum}")
"#,
        "30",
    );
}

// ==================== Nested Tuples ====================

#[test]
fn test_tuple_nested() {
    assert_runs_with_output(
        r#"
use system.io

let t = ((1, 2), (3, 4))
let sum = t.0.0 + t.0.1 + t.1.0 + t.1.1
println(f"{sum}")
"#,
        "10",
    );
}

// ==================== Managed Types ====================

#[test]
fn test_tuple_with_managed_types() {
    assert_runs(
        r#"
use system.collections.list

// If this crashes, there's a problem with tuple drop code.
// Tuples shouldn't leak memory (verified by leak sanitizer / Miri internal RC checks if any).
let t = (List([1, 2, 3]), "hello")
let l2 = t.0 // Increase RC
let s2 = t.1 // Increase RC
"#,
    );
}
