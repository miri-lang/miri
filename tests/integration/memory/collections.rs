// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for nested and complex collection memory management.
// Nested collections hold managed sub-objects; each layer must be
// IncRef'd and DecRef'd independently.

use super::super::utils::*;

#[test]
fn test_list_of_lists_no_leak() {
    // Inner Lists are managed objects; outer List must DecRef each on drop.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let inner1 = List([1, 2])
    let inner2 = List([3, 4])
    let inner3 = List([5, 6])
    let outer = List([inner1, inner2, inner3])
    println(f"{outer.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_of_lists_aliased_inner_no_double_free() {
    // Inner list shared between outer container and standalone variable.
    // After outer is dropped inner must still have RC = 1.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let shared_inner = List([10, 20, 30])
    var outer = List([shared_inner])  // outer holds alias, RC(inner)=2
    outer = List([])                  // old outer dropped, RC(inner)=1
    println(f"{shared_inner.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_push_many_then_drop() {
    // Grow a list via push; all pushed managed values must be freed on drop.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([List([1]), List([2]), List([3])])
    // Seed with a typed element so big is List<List<int>>, not List<void>
    var big = List([items.element_at(0)])
    var i = 1
    while i < items.length()
        let sub = items.element_at(i)
        big.push(sub)
        i += 1
    println(f"{big.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_clear_then_drop() {
    // After clear() the backing list has no elements; drop of the outer shell
    // must not attempt to free ghost elements.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([List([1, 2]), List([3, 4])])
    l.clear()
    println(f"{l.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_remove_at_frees_element() {
    // remove_at must DecRef the removed element.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([List([1, 2, 3]), List([4, 5])])
    l.remove_at(0)
    println(f"{l.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_with_list_values_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    let m = {"nums": List([1, 2, 3]), "more": List([4, 5])}
    println(f"{m.length()}")
"#,
        "2",
    );
}

#[test]
fn test_map_set_replaces_value_frees_old() {
    // Overwriting an existing key must DecRef the old value list.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    var m = {"k": List([1, 2, 3])}
    m.set("k", List([99]))
    let v = m["k"]   // index operator returns direct value (not Option)
    println(f"{v.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_remove_key_frees_value() {
    // Removing a key must DecRef the associated value.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    var m = {"a": List([1]), "b": List([2, 3])}
    m.remove("a")
    println(f"{m.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_clear_frees_all_values() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    var m = {"x": List([1, 2]), "y": List([3]), "z": List([4, 5, 6])}
    m.clear()
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_map_with_class_values_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Point
    var x int
    var y int

fn main()
    let m = {"origin": Point(x: 0, y: 0), "tip": Point(x: 3, y: 4)}
    println(f"{m.length()}")
"#,
        "2",
    );
}

#[test]
fn test_set_add_remove_no_leak() {
    // add + remove pairs must balance IncRef/DecRef.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {1, 2, 3}
    s.add(4)
    s.remove(2)
    println(f"{s.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_clear_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {10, 20, 30, 40}
    s.clear()
    println(f"{s.length()}")
"#,
        "0",
    );
}

#[test]
fn test_array_of_lists_no_leak() {
    // Fixed-size Array whose elements are managed Lists.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.array
use system.collections.list

fn main()
    let arr = [List([1, 2]), List([3, 4, 5])]
    println(f"{arr.length()}")
"#,
        "2",
    );
}

#[test]
fn test_three_level_list_nesting_no_leak() {
    // List<List<List<int>>>: three independent RC layers.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let deep = List([List([List([1, 2]), List([3])]), List([List([4, 5, 6])])])
    println(f"{deep.length()}")
"#,
        "2",
    );
}
