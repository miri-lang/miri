// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Custom elem_drop_fn (task 2.4b) ──────────────────────────────────────────

#[test]
fn test_list_of_custom_clear_no_crash() {
    // List<Point>: __decref_Point must be set as elem_drop_fn so that clear()
    // properly DecRefs each Point instance instead of leaking it.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Point
    var x int
    var y int

fn main()
    var pts = List([Point(x: 1, y: 2), Point(x: 3, y: 4)])
    pts.clear()
    println(f"{pts.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_of_custom_remove_at_no_crash() {
    // remove_at on List<Point> must call __decref_Point on the removed element.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Item
    var value int

fn main()
    var items = List([Item(value: 10), Item(value: 20), Item(value: 30)])
    items.remove_at(1)
    println(f"{items.length()}")
"#,
        "2",
    );
}

#[test]
fn test_list_of_custom_aliased_element_outlives_clear() {
    // Pull an element reference before clearing the list; element must survive
    // (RC still > 0) while the rest are freed.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Node
    var id int

fn main()
    var nodes = List([Node(id: 1), Node(id: 2), Node(id: 3)])
    let kept = nodes.element_at(0)
    nodes.clear()
    println(f"{kept.id}")
"#,
        "1",
    );
}

// ── List<List<int>> scope-exit cleanup (task 2.5) ────────────────────────────

#[test]
fn test_list_of_lists_out_of_scope_no_crash() {
    // List<List<int>> going out of scope must DecRef each inner list.
    // Different from array/rc.rs which tests the Array<List<T>> constructor
    // path; this exercises standalone List<List<int>> scope exit.
    assert_runs(
        r#"
use system.collections.list

fn make()
    let outer = List([List([1, 2]), List([3, 4]), List([5])])
    // outer goes out of scope — inner lists must be DecRef'd

fn main()
    make()
    make()
"#,
    );
}

// ── Drop-fn setter wiring (task 2.4) ─────────────────────────────────────────

#[test]
fn test_list_of_strings_clear_no_crash() {
    // List<String>: elem_drop_fn must be set so that clear() properly DecRefs
    // each string element instead of leaking them.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List(["hello", "world", "foo"])
    l.clear()
    println(f"{l.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_of_strings_remove_no_crash() {
    // remove_at on a List<String> must call the elem_drop_fn on the removed element.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List(["a", "b", "c"])
    l.remove_at(1)
    println(f"{l.length()}")
"#,
        "2",
    );
}

#[test]
fn test_list_of_strings_out_of_scope_no_crash() {
    // A List<String> going out of scope must free string elements without crashing.
    assert_runs(
        r#"
use system.collections.list

fn make() [String]
    List(["x", "y", "z"])

fn main()
    let _ = make()
    // list goes out of scope here, strings freed
"#,
    );
}

#[test]
fn test_list_alias_no_double_free() {
    assert_runs(
        "
use system.collections.list
let l1 = List([1, 2, 3])
let l2 = l1 // IncRef
// Both out of scope, safe drop
",
    );
}

#[test]
fn test_list_reassign_frees_old() {
    assert_runs(
        "
use system.collections.list
var l = List([1, 2, 3])
l = List([4, 5])
",
    );
}

#[test]
fn test_list_passed_to_function_no_dangle() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn consume(l [int])
    // goes out of scope

fn main()
    let l = List([10, 20, 30])
    consume(l)
    println(f\"{l.length()}\")
",
        "3",
    );
}

#[test]
fn test_list_returned_from_function_with_rc() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

fn make_and_alias() [int]
    let l = List([1, 2, 3])
    let alias = l
    return alias

fn main()
    let l = make_and_alias()
    println(f\"{l.length()}\")
",
        "3",
    );
}

#[test]
fn list_reference_semantics_mutation() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l1 = List([10, 20, 30])
let l2 = l1
l1.push(40)
println(f\"{l2.length()}\")
println(f\"{l2[3]}\")
",
        "4\n40",
    );
}

// ── Push/insert incref for managed elements (task 3.1) ───────────────────────

#[test]
fn test_list_push_managed_val_incref() {
    // push a managed String variable; after the local goes out of scope the list
    // must still hold a valid reference (IncRef'd at push time).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn push_string() [String]
    let s = "hel" + "lo"
    var l = List(["first"])
    l.push(s)
    return l
    // s goes out of scope here — list must still own "hello"

fn main()
    let l = push_string()
    println(l[1])
"#,
        "hello",
    );
}

#[test]
fn test_list_insert_managed_val_incref() {
    // insert a managed String variable at index 0; after local goes out of scope
    // the list must still hold a valid reference.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn insert_string() [String]
    let s = "wor" + "ld"
    var l = List(["first"])
    l.insert(0, s)
    return l
    // s goes out of scope here

fn main()
    let l = insert_string()
    println(l[0])
"#,
        "world",
    );
}
