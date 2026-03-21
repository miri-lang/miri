// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for temporaries: managed objects whose lifetime is a single statement
// or sub-expression.  These must be DecRef'd immediately after use, not held
// until the enclosing scope exits.

use super::super::utils::*;

#[test]
fn test_inline_list_as_function_arg_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn count(l [int]) int
    l.length()

fn main()
    println(f"{count(List([1, 2, 3]))}")
"#,
        "3",
    );
}

#[test]
fn test_inline_map_as_function_arg_no_leak() {
    // Map literals can't be nested inside f-strings; extract to a local.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn size(m Map<String, int>) int
    m.length()

fn main()
    let m = {"a": 1, "b": 2, "c": 3}
    println(f"{size(m)}")
"#,
        "3",
    );
}

#[test]
fn test_inline_class_instance_as_function_arg_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn show(p Point) int
    p.x + p.y

fn main()
    println(f"{show(Point(x: 3, y: 4))}")
"#,
        "7",
    );
}

#[test]
fn test_chained_method_on_inline_list_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    println(f"{List([5, 3, 1, 4, 2]).length()}")
"#,
        "5",
    );
}

#[test]
fn test_inline_list_in_loop_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn count(l [int]) int
    l.length()

fn main()
    var total = 0
    for i in 0..10
        total = total + count(List([i]))
    println(f"{total}")
"#,
        "10",
    );
}

#[test]
fn test_temporary_in_fstring_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    println(f"{List([10, 20, 30]).length()}")
"#,
        "3",
    );
}

#[test]
fn test_two_inline_lists_as_args_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn combine_length(a [int], b [int]) int
    a.length() + b.length()

fn main()
    println(f"{combine_length(List([1, 2]), List([3, 4, 5]))}")
"#,
        "5",
    );
}

#[test]
fn test_temporary_in_conditional_branch_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let flag = 1
    var result = 0
    if flag == 1
        result = List([1, 2, 3]).length()
    else
        result = List([0]).length()
    println(f"{result}")
"#,
        "3",
    );
}

#[test]
fn test_temporary_bound_and_used_no_leak() {
    // Binding a freshly constructed object then immediately consuming it.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let t = List([1, 2, 3, 4])
    println(f"{t.length()}")
"#,
        "4",
    );
}

#[test]
fn test_inline_class_with_list_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Payload
    var data [int]

fn measure(p Payload) int
    p.data.length()

fn main()
    println(f"{measure(Payload(data: List([1, 2, 3, 4, 5])))}")
"#,
        "5",
    );
}
