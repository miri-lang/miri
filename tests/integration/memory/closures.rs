// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness around lambda captures of managed values.
// A lambda that captures a managed value must IncRef it on creation and
// DecRef it when the lambda itself is dropped.
//
// Covered cases:
//   - Primitive captures (int, float, bool) — copy semantics, no RC needed
//   - String captures — pointer capture with correct RC
//   - Class captures (primitive fields and List fields)
//   - List captures — closure env DecRefs captured List on drop (fixed in Milestone 5 Task 5.1)

use super::super::utils::*;

// ────────────────────────────────────────────────────────────────────────────
//  Working captures
// ────────────────────────────────────────────────────────────────────────────

/// Lambda captures a String; RC must be balanced across calls and drop.
/// This extends the basic capture test with an explicit memory-leak assertion.
#[test]
fn test_lambda_captures_string_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let greeting = "hello"
    let greet = fn() String: greeting
    println(greet())
    println(greet())
"#,
        "hello\nhello",
    );
}

/// Lambda captures a class instance whose fields are all primitives.
/// The class pointer must be IncRef'd on lambda creation and DecRef'd on drop.
#[test]
fn test_lambda_captures_class_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var value int

fn main()
    let c = Counter(value: 42)
    let get = fn() int: c.value
    println(f"{get()}")
"#,
        "42",
    );
}

/// Lambda captures two Strings; both must be freed when the lambda drops.
#[test]
fn test_lambda_captures_two_strings_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let first = "hello"
    let second = "world"
    let both = fn() String: first
    println(both())
    println(second)
"#,
        "hello\nworld",
    );
}

/// Lambda called multiple times with a captured String; no double-free.
#[test]
fn test_lambda_captures_string_called_multiple_times_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let label = "item"
    let print_label = fn() String: label
    println(print_label())
    println(print_label())
    println(print_label())
"#,
        "item\nitem\nitem",
    );
}

// ────────────────────────────────────────────────────────────────────────────
//  Collection captures (Milestone 5 Task 0e — now fixed)
// ────────────────────────────────────────────────────────────────────────────

/// Lambda capturing a List; the List must not be freed while the lambda is live,
/// and must be freed exactly once after both the lambda and outer binding drop.
#[test]
fn test_lambda_captures_list_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    let size = fn() int: items.length()
    println(f"{size()}")
"#,
        "3",
    );
}

/// Lambda called multiple times; the captured List must not be double-freed.
#[test]
fn test_lambda_called_multiple_times_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let data = List([10, 20, 30])
    let len = fn() int: data.length()
    println(f"{len()}")
    println(f"{len()}")
    println(f"{len()}")
"#,
        "3\n3\n3",
    );
}

/// Lambda captures two managed Lists; both must be freed when the lambda drops.
#[test]
fn test_lambda_captures_two_lists_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2])
    let b = List([3, 4, 5])
    let combined = fn() int: a.length() + b.length()
    println(f"{combined()}")
"#,
        "5",
    );
}

/// Lambda created fresh inside a loop, capturing the same outer List each time.
#[test]
fn test_lambda_created_in_loop_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let base = List([1, 2, 3])
    var total = 0
    for i in 0..4
        let f = fn() int: base.length()
        total = total + f()
    println(f"{total}")
    println(f"{base.length()}")
"#,
        "12\n3",
    );
}

/// Lambda passed as argument to a higher-order function, captures a List.
#[test]
fn test_lambda_passed_as_arg_captures_list_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn apply(f fn() int) int
    f()

fn main()
    let items = List([1, 2, 3, 4])
    let result = apply(fn() int: items.length())
    println(f"{result}")
"#,
        "4",
    );
}

/// Lambda captures a class with a managed List field.
#[test]
fn test_lambda_captures_class_with_list_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Bag
    var items [int]

fn main()
    let bag = Bag(items: List([5, 10, 15]))
    let bag_len = fn() int: bag.items.length()
    println(f"{bag_len()}")
    println(f"{bag.items.length()}")
"#,
        "3\n3",
    );
}

/// Mutable closure var reassigned from a capturing closure to a non-capturing one.
/// The stale `closure_capture_types` entry for the original local must not cause
/// a spurious DecRef on the new closure's payload (which has no captures).
#[test]
fn test_closure_var_reassigned_capturing_to_non_capturing() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let data = List([1, 2, 3])
    var f = fn() int: data.length()
    f = fn() int: 99
    println(f"{f()}")
"#,
        "99",
    );
}

/// Mutable closure var reassigned from a non-capturing closure to a capturing one.
/// Ensures the new captures are correctly tracked and freed on drop.
#[test]
fn test_closure_var_reassigned_non_capturing_to_capturing() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let data = List([10, 20, 30])
    var f = fn() int: 42
    f = fn() int: data.length()
    println(f"{f()}")
"#,
        "3",
    );
}

/// Two independent lambdas each capture the same List; dropping both lambdas
/// must not double-free the List.
#[test]
fn test_two_lambdas_capture_same_list_no_double_free() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let pool = List([7, 8, 9])
    let f1 = fn() int: pool.length()
    let f2 = fn() int: pool.length()
    println(f"{f1() + f2()}")
"#,
        "6",
    );
}
