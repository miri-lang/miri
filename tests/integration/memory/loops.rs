// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for correct memory management inside loops.
// Objects created per-iteration must be dropped at iteration end,
// not accumulated until after the loop.

use super::super::utils::*;

#[test]
fn test_while_loop_list_per_iteration_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var i = 0
    while i < 10
        let tmp = List([i, i + 1, i + 2])
        i += 1
    println("done")
"#,
        "done",
    );
}

#[test]
fn test_while_loop_class_per_iteration_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io

class Event
    var id int

fn main()
    var i = 0
    while i < 20
        let e = Event(id: i)
        i += 1
    println("done")
"#,
        "done",
    );
}

#[test]
fn test_while_loop_class_with_list_field_per_iteration() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Frame
    var data [int]

fn main()
    var i = 0
    while i < 5
        let f = Frame(data: List([i, i * 2, i * 3]))
        i += 1
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_while_loop_managed_variable_reassigned_each_iter() {
    // Reassigning a managed variable inside a loop must free the previous value
    // at each iteration, not just at end of loop.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([0])
    var i = 0
    while i < 10
        l = List([i])   // old l freed each time
        i += 1
    println(f"{l.length()}")
"#,
        "1",
    );
}

#[test]
fn test_for_range_list_per_iteration_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    for i in 0..50
        let tmp = List([i])
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_for_range_accumulating_into_outer_list() {
    // Build a list across iterations; only the outer list should be live at the end.
    // Seed with one typed element so the List is List<int> rather than List<void>.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var result = List([0])
    for i in 1..5
        result.push(i)
    println(f"{result.length()}")
"#,
        "5",
    );
}

#[test]
fn test_nested_while_loops_managed_objects_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var i = 0
    while i < 4
        var j = 0
        while j < 4
            let tmp = List([i, j])
            j += 1
        i += 1
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_nested_for_loops_with_class_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Cell
    var row int
    var col int

fn main()
    for r in 0..3
        for c in 0..3
            let cell = Cell(row: r, col: c)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_for_over_list_of_classes_no_leak() {
    // Iterating with element_at bumps RC; must be DecRef'd when loop var dies.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Item
    var v int

fn main()
    let items = List([Item(v: 1), Item(v: 2), Item(v: 3)])
    var total = 0
    for item in items
        total = total + item.v
    println(f"{total}")
"#,
        "6",
    );
}

#[test]
fn test_for_over_list_of_lists_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let matrix = List([List([1, 2]), List([3, 4]), List([5, 6])])
    var total = 0
    for row in matrix
        for i in 0..row.length()
            total = total + row.element_at(i)
    println(f"{total}")
"#,
        "21",
    );
}

#[test]
fn test_for_over_map_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m = {"a": 1, "b": 2, "c": 3}
    var count = 0
    for i in 0..m.length()
        let v = m.value_at(i)
        count = count + v
    println(f"{count}")
"#,
        "6",
    );
}

#[test]
fn test_while_loop_inner_alias_of_outer_no_double_free() {
    // Taking an alias of an outer List each iteration; alias drops at
    // iteration end, outer List must not be freed prematurely.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let outer = List([1, 2, 3])
    var i = 0
    while i < 5
        let alias = outer   // IncRef
        i += 1
        // alias drops → DecRef; outer RC returns to 1
    println(f"{outer.length()}")
"#,
        "3",
    );
}
