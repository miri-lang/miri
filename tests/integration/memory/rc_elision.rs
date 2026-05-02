// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Runtime correctness tests for the RC elision optimization.
//
// RC elision removes redundant (IncRef, DecRef) pairs from linear flows.
// These tests verify that programs still compute correct results and have
// no memory leaks or use-after-free after the elision pass runs.

use super::super::utils::*;

/// Linear element access: elision removes the IncRef/DecRef pair on the
/// temporary copy used for element_at, but the result must still be correct.
#[test]
fn test_linear_element_access_correct() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([10, 20, 30])
    let x = items.element_at(0)
    let y = items.element_at(2)
    println(f"{x + y}")
"#,
        "40",
    );
}

/// Multiple sequential element accesses — each pair is elided independently.
#[test]
fn test_multiple_linear_accesses_correct() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([1, 2, 3, 4, 5])
    let a = items.element_at(0)
    let b = items.element_at(1)
    let c = items.element_at(2)
    let d = items.element_at(3)
    let e = items.element_at(4)
    println(f"{a + b + c + d + e}")
"#,
        "15",
    );
}

/// Element access via function parameter — the parameter's RC is managed by
/// the caller, and elision must not corrupt it.
#[test]
fn test_param_element_access_no_corruption() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn first(items [int]) int:
    return items.element_at(0)

fn second(items [int]) int:
    return items.element_at(1)

fn main()
    let items = List([42, 99])
    let a = first(items)
    let b = second(items)
    println(f"{a} {b}")
"#,
        "42 99",
    );
}

/// Aliased list: after `let copy = items`, both `items` and `copy` are live.
/// The program must produce correct results regardless of whether the elision
/// pass fires on the copy pair — the list must still be live for both accesses.
#[test]
fn test_aliased_list_correct() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([7, 14, 21])
    let copy = items
    let x = copy.element_at(0)
    let y = items.element_at(2)
    println(f"{x} {y}")
"#,
        "7 21",
    );
}

/// Resource type (has destructor): elision must skip the pair so the
/// destructor fires correctly when the variable goes out of scope.
#[test]
fn test_resource_type_destructor_fires() {
    assert_runs_with_output(
        r#"
use system.io

struct Counter
    value int
    fn drop(self)
        println("dropped")

fn main()
    let c = Counter(value: 42)
    let v = c.value
    println(f"{v}")
"#,
        "42\ndropped",
    );
}

/// No memory leaks after linear element accesses (MIRI_LEAK_CHECK).
#[test]
fn test_linear_access_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let xs = List([1, 2, 3])
    let x = xs.element_at(1)
    println(f"{x}")
"#,
        "2",
    );
}

/// No memory leaks when passing a list through multiple functions linearly.
#[test]
fn test_linear_pass_through_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn get_elem(items [int], idx int) int:
    return items.element_at(idx)

fn sum_ends(items [int]) int:
    let first = get_elem(items, 0)
    let last = get_elem(items, 2)
    return first + last

fn main()
    let xs = List([100, 200, 300])
    let result = sum_ends(xs)
    println(f"{result}")
"#,
        "400",
    );
}

/// String linear access — strings are also managed types, elision applies.
#[test]
fn test_string_linear_access_correct() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn greet(name String) String:
    return f"Hello, {name}!"

fn main()
    let msg = greet("World")
    println(msg)
"#,
        "Hello, World!",
    );
}

/// Mix of linear and aliased access in the same function.
/// Linear pair must be elided; aliased pair must not be.
#[test]
fn test_mixed_linear_and_aliased_correct() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    // linear access — pair elided
    let x = items.element_at(0)
    // aliased: both alias and items used afterwards
    let alias = items
    let y = alias.element_at(1)
    let z = items.element_at(2)
    println(f"{x} {y} {z}")
"#,
        "1 2 3",
    );
}
