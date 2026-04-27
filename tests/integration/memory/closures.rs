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

// ────────────────────────────────────────────────────────────────────────────
//  Closure allocation tracking (Milestone 5 Task 5.4)
// ────────────────────────────────────────────────────────────────────────────

/// Verifies that closure allocations are counted in CLOSURE_ALLOC_BALANCE and
/// properly balanced by closure frees: a simple closure that is created and
/// dropped must leave the counter at zero, or the MIRI_LEAK_CHECK atexit handler
/// would fire and this test would fail with "Memory leak detected".
///
/// This is the positive half of the 5.4 acceptance criterion: the tracking is
/// in place AND does not produce false positives on correct code.
#[test]
fn test_closure_alloc_tracked_no_false_positive() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let f = fn() int: 42
    println(f"{f()}")
"#,
        "42",
    );
}

/// E2E: a simulated closure leak (via `system.testing.simulate_closure_leak`)
/// must cause the MIRI_LEAK_CHECK atexit handler to fire and exit non-zero with
/// the "leaked N closure allocation(s)" message.
///
/// This is the negative half of the 5.4 acceptance criterion: the tracking
/// correctly catches an imbalanced closure counter at process exit.
#[test]
fn test_closure_leak_detector_fires() {
    assert_leak_detected(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
"#,
        "leaked 1 closure allocation(s)",
    );
}

/// Two simulated leaks must report a count of 2.
#[test]
fn test_closure_leak_detector_reports_count() {
    assert_leak_detected(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
    simulate_closure_leak()
"#,
        "leaked 2 closure allocation(s)",
    );
}

/// Same with a capturing closure to verify that both the closure struct and its
/// captured managed value remain balanced across the full alloc → use → drop cycle.
#[test]
fn test_capturing_closure_alloc_tracked_no_false_positive() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    let f = fn() int: items.length()
    println(f"{f()}")
"#,
        "3",
    );
}

// ────────────────────────────────────────────────────────────────────────────
//  Closure returned from a function (Milestone 5 Task 5.5)
// ────────────────────────────────────────────────────────────────────────────

/// Closure created inside a function and returned to the caller.
/// The captured List must survive the creator's scope exit (IncRef at capture)
/// and be freed exactly once when the returned closure is dropped by the caller.
#[test]
fn test_closure_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_counter(items [int]) fn() int
    fn() int: items.length()

fn main()
    let result = make_counter(List([1, 2, 3]))()
    println(f"{result}")
"#,
        "3",
    );
}

/// Return a closure that captures a locally-created List (not a parameter).
#[test]
fn test_closure_returned_captures_local_list() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_counter() fn() int
    let items = List([1, 2, 3])
    fn() int: items.length()

fn main()
    let counter = make_counter()
    let result = counter()
    println(f"{result}")
"#,
        "3",
    );
}

/// Return a non-capturing closure from a function.
#[test]
fn test_closure_returned_no_capture() {
    assert_runs_with_output(
        r#"
use system.io

fn make_fn() fn() int
    fn() int: 42

fn main()
    let f = make_fn()
    let result = f()
    println(f"{result}")
"#,
        "42",
    );
}

/// Return a closure that captures a primitive parameter.
#[test]
fn test_closure_returned_captures_int_param() {
    assert_runs_with_output(
        r#"
use system.io

fn make_adder(n int) fn() int
    fn() int: n + 1

fn main()
    let f = make_adder(10)
    let result = f()
    println(f"{result}")
"#,
        "11",
    );
}

/// Debug: check list length inside make_counter vs after.
#[test]
fn test_debug_list_param_capture_length() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_counter(items [int]) fn() int
    println(f"inside:{items.length()}")
    fn() int: items.length()

fn main()
    let counter = make_counter(List([1, 2, 3]))
    let result = counter()
    println(f"outside:{result}")
"#,
        "inside:3\noutside:3",
    );
}

/// Closure captures a class instance that has a managed (List) field.
/// When the closure drops, the class's RC must reach 0 and the List field
/// must be freed via the drop thunk — not just via a bare libc::free.
#[test]
fn test_closure_captures_class_with_list_field() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Counter
    items [int]

fn make_reader(c Counter) fn() int
    fn() int: c.items.length()

fn main()
    let c = Counter(List([10, 20, 30]))
    let reader = make_reader(c)
    println(f"{reader()}")
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
