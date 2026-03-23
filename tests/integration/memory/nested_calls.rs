// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness when managed objects flow through nested
// function calls.  Temporaries returned by inner calls are the arguments
// to outer calls; each layer must IncRef on entry and DecRef on exit with
// no net leaks or double-frees.

use super::super::utils::*;

/// f(g(1), h(2)) where g and h each *create* a new List from a scalar.
/// Two managed values produced by inner calls are consumed by the outer call;
/// both must be freed after combine() returns.
#[test]
fn test_nested_call_two_managed_intermediates_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_single(n int) [int]
    List([n])

fn make_pair(n int) [int]
    List([n, n + 1])

fn combine(a [int], b [int]) int
    a.length() + b.length()

fn main()
    println(f"{combine(make_single(1), make_pair(2))}")
"#,
        "3",
    );
}

/// Three managed arguments each produced by a separate inner call.
#[test]
fn test_nested_call_three_managed_args_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn single(n int) [int]
    List([n])

fn sum3(a [int], b [int], c [int]) int
    a.length() + b.length() + c.length()

fn main()
    println(f"{sum3(single(1), single(2), single(3))}")
"#,
        "3",
    );
}

/// count(make(n)) where make creates a new List each call — verifies that
/// a managed return value from an inner call is correctly freed after use.
#[test]
fn test_nested_call_inner_makes_outer_consumes_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make(n int) [int]
    List([n, n + 1, n + 2])

fn count(l [int]) int
    l.length()

fn main()
    println(f"{count(make(10))}")
    println(f"{count(make(20))}")
"#,
        "3\n3",
    );
}

/// Triple nesting: outer(middle(inner(List))).
#[test]
fn test_nested_call_triple_chain_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make(n int) [int]
    List([n, n + 1, n + 2])

fn count(l [int]) int
    l.length()

fn doubled(n int) int
    n * 2

fn main()
    println(f"{doubled(count(make(10)))}")
"#,
        "6",
    );
}

/// Class returned from one function used directly as argument to another.
#[test]
fn test_nested_call_class_through_two_hops_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Rect
    var w int
    var h int

fn make_rect(w int, h int) Rect
    Rect(w: w, h: h)

fn area(r Rect) int
    r.w * r.h

fn main()
    println(f"{area(make_rect(3, 4))}")
    println(f"{area(make_rect(5, 6))}")
"#,
        "12\n30",
    );
}

/// Class with a managed List field threaded through two function hops.
#[test]
fn test_nested_call_class_with_list_field_through_calls_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Bag
    var items [int]

fn make_bag(n int) Bag
    Bag(items: List([n, n + 1]))

fn bag_size(b Bag) int
    b.items.length()

fn main()
    println(f"{bag_size(make_bag(10))}")
    println(f"{bag_size(make_bag(20))}")
"#,
        "2\n2",
    );
}

/// Managed return values from two independent nested calls combined inline.
/// count(make_a()) + count(make_b()) — each make_* creates a temporary List.
#[test]
fn test_nested_call_two_results_combined_inline_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_a() [int]
    List([1, 2, 3])

fn make_b() [int]
    List([10, 20])

fn count(l [int]) int
    l.length()

fn main()
    println(f"{count(make_a()) + count(make_b())}")
"#,
        "5",
    );
}

/// add(size(make_a()), size(make_b())) — four function calls in a tree,
/// each leaf creates a managed value, each internal node consumes and returns int.
#[test]
fn test_nested_call_four_function_tree_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make_a() [int]
    List([1, 2, 3, 4])

fn make_b() [int]
    List([10, 20])

fn size(l [int]) int
    l.length()

fn add(a int, b int) int
    a + b

fn main()
    println(f"{add(size(make_a()), size(make_b()))}")
"#,
        "6",
    );
}

/// Nested calls inside a loop — per-iteration temporaries must not accumulate.
#[test]
fn test_nested_call_in_loop_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make(n int) [int]
    List([n])

fn count(l [int]) int
    l.length()

fn main()
    var total = 0
    for i in 0..10
        total = total + count(make(i))
    println(f"{total}")
"#,
        "10",
    );
}
