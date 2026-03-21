// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness across function boundaries:
// passing managed values into functions, returning them, consuming them,
// and deep call chains.

use super::super::utils::*;

#[test]
fn test_list_passed_and_returned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn identity(l [int]) [int]
    l

fn main()
    let l = List([1, 2, 3])
    let r = identity(l)
    println(f"{r.length()}")
"#,
        "3",
    );
}

#[test]
fn test_map_passed_and_returned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn identity(m Map<String, int>) Map<String, int>
    m

fn main()
    let m = {"a": 1, "b": 2}
    let r = identity(m)
    println(f"{r.length()}")
"#,
        "2",
    );
}

#[test]
fn test_list_consumed_in_callee_caller_alias_survives() {
    // Caller keeps an alias; callee's copy drops at function exit, leaving RC=1.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn sink(l [int])
    // l drops here
    let _n = l.length()

fn main()
    let mine = List([7, 8, 9])
    sink(mine)
    println(f"{mine.length()}")
"#,
        "3",
    );
}

#[test]
fn test_class_consumed_in_callee_caller_alias_survives() {
    assert_runs_with_output(
        r#"
use system.io

class Node
    var v int

fn inspect(n Node) int
    n.v

fn main()
    let n = Node(v: 42)
    let result = inspect(n)
    println(f"{result}")
    println(f"{n.v}")
"#,
        "42\n42",
    );
}

#[test]
fn test_list_passed_to_two_functions_sequentially_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn count(l [int]) int
    l.length()

fn sum_first_two(l [int]) int
    l.element_at(0) + l.element_at(1)

fn main()
    let l = List([10, 20, 30])
    println(f"{count(l)}")
    println(f"{sum_first_two(l)}")
"#,
        "3\n30",
    );
}

#[test]
fn test_deeply_nested_calls_with_managed_return_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn level3() [int]
    List([1, 2, 3])

fn level2() [int]
    level3()

fn level1() [int]
    level2()

fn main()
    let l = level1()
    println(f"{l.length()}")
"#,
        "3",
    );
}

#[test]
fn test_class_built_in_nested_calls_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Tree
    var value int
    var children [int]

fn make_leaf(v int) Tree
    Tree(value: v, children: List([]))

fn make_node(v int) Tree
    let child = make_leaf(v * 2)
    Tree(value: v, children: List([child.value]))

fn main()
    let t = make_node(5)
    println(f"{t.value}")
    println(f"{t.children.length()}")
"#,
        "5\n1",
    );
}

#[test]
fn test_function_return_used_inline_no_leak() {
    // Result of function call used directly as argument to another call;
    // the temporary must be DecRef'd after use.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn make() [int]
    List([1, 2, 3, 4, 5])

fn count(l [int]) int
    l.length()

fn main()
    println(f"{count(make())}")
"#,
        "5",
    );
}

#[test]
fn test_recursive_function_managed_locals_no_leak() {
    // Each recursive call creates and drops a local List; no accumulation.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn countdown(n int) int
    if n <= 0
        return 0
    let tmp = List([n])   // local managed object; must drop at end of call
    countdown(n - 1) + tmp.length()

fn main()
    println(f"{countdown(5)}")
"#,
        "5",
    );
}

#[test]
fn test_early_return_drops_locals_no_leak() {
    // A managed object created before an early return must still be freed.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn process(flag int) int
    let l = List([1, 2, 3])
    if flag == 0
        return 0       // l must be dropped here
    l.length()

fn main()
    println(f"{process(0)}")
    println(f"{process(1)}")
"#,
        "0\n3",
    );
}
