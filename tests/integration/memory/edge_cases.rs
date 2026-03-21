// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Edge-case memory tests: empty collections, single-element collections,
// large numbers of items, conditional allocation, and string interactions.

use super::super::utils::*;

#[test]
fn test_empty_list_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let l = List([])
    println(f"{l.length()}")
"#,
        "0",
    );
}

#[test]
fn test_empty_map_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m = Map<String, int>()
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_push_then_clear_no_leak() {
    // Seed with a typed element so the List is List<int> not List<void>,
    // then grow and clear; all elements must be freed.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([0])
    l.push(1)
    l.push(2)
    l.push(3)
    l.clear()
    println(f"{l.length()}")
"#,
        "0",
    );
}

#[test]
fn test_empty_map_set_then_clear_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<String, int>()
    m.set("a", 1)
    m.set("b", 2)
    m.clear()
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_single_element_list_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let l = List([42])
    println(f"{l.length()}")
    println(f"{l.element_at(0)}")
"#,
        "1\n42",
    );
}

#[test]
fn test_single_element_list_pop_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([7])
    let v = l.pop()
    println(f"{v}")
    println(f"{l.length()}")
"#,
        "7\n0",
    );
}

#[test]
fn test_large_list_no_leak() {
    // Seed with one typed element, push 999 more; 1000 total.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([0])
    for i in 1..1000
        l.push(i)
    println(f"{l.length()}")
"#,
        "1000",
    );
}

#[test]
fn test_large_list_of_classes_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Item
    var v int

fn main()
    var l = List([Item(v: 0)])
    for i in 1..200
        l.push(Item(v: i))
    println(f"{l.length()}")
"#,
        "200",
    );
}

#[test]
fn test_large_map_no_leak() {
    // Stress the map with many insertions; all must be freed at drop.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<int, int>()
    for i in 0..100
        m.set(i, i * 2)
    println(f"{m.length()}")
"#,
        "100",
    );
}

#[test]
fn test_managed_object_created_only_in_true_branch_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn maybe_alloc(flag int) int
    if flag == 1
        let l = List([1, 2, 3])
        return l.length()
    0

fn main()
    println(f"{maybe_alloc(1)}")
    println(f"{maybe_alloc(0)}")
"#,
        "3\n0",
    );
}

#[test]
fn test_managed_object_created_only_in_false_branch_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn maybe_alloc(flag int) int
    if flag == 1
        return 0
    let l = List([10, 20])
    l.length()

fn main()
    println(f"{maybe_alloc(1)}")
    println(f"{maybe_alloc(0)}")
"#,
        "0\n2",
    );
}

#[test]
fn test_list_of_strings_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let words = List(["hello", "world", "foo"])
    println(f"{words.length()}")
"#,
        "3",
    );
}

#[test]
fn test_class_with_string_and_list_in_loop_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Entry
    var name String
    var values [int]

fn main()
    for i in 0..5
        let e = Entry(name: "entry", values: List([i, i + 1]))
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn test_map_string_keys_clear_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {"hello": 1, "world": 2, "foo": 3}
    m.clear()
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_pop_until_empty_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([1, 2, 3, 4, 5])
    while l.length() > 0
        let _ = l.pop()
    println(f"{l.length()}")
"#,
        "0",
    );
}

#[test]
fn test_list_of_optional_values_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class MaybeInt
    var has_value int
    var value int

fn main()
    let items = List([MaybeInt(has_value: 1, value: 10), MaybeInt(has_value: 0, value: 0), MaybeInt(has_value: 1, value: 30)])
    var total = 0
    for item in items
        if item.has_value == 1
            total = total + item.value
    println(f"{total}")
"#,
        "40",
    );
}
