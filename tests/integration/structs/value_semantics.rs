// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A struct is a value: a write through one name is never seen through another.
//!
//! A struct with a managed field is reference counted, so a second name shares
//! its block until one of them writes; the write copies a block anything else
//! still holds. Each test reads back every name that held the value, under the
//! heap guard, so a copy that frees or leaks the original fails as loudly as one
//! that never happens.

use super::utils::assert_heap_guard_output;

const BOX: &str = r#"
use system.io

struct Box
    count int
    name String
"#;

fn with_box(main: &str) -> String {
    format!("{BOX}\n{main}")
}

#[test]
fn test_struct_rebound_and_captured_keeps_each_value() {
    assert_heap_guard_output(
        &with_box(
            r#"
fn main()
    var b = Box(count: 1, name: "x")
    let f = fn() int: b.count
    b.count = 5
    var c = b
    c.count = 9
    println(f"{f()} {b.count} {c.count}")
"#,
        ),
        "1 5 9",
    );
}

#[test]
fn test_write_to_the_original_leaves_the_copy_alone() {
    assert_heap_guard_output(
        &with_box(
            r#"
fn main()
    var b = Box(count: 1, name: "x")
    var c = b
    b.count = 2
    b.name = "y"
    println(f"{b.count} {b.name} {c.count} {c.name}")
"#,
        ),
        "2 y 1 x",
    );
}

#[test]
fn test_compound_write_to_a_shared_struct_copies_it() {
    assert_heap_guard_output(
        &with_box(
            r#"
fn main()
    var b = Box(count: 1, name: "x")
    var c = b
    c.count += 10
    println(f"{b.count} {c.count}")
"#,
        ),
        "1 11",
    );
}

#[test]
fn test_struct_passed_to_a_function_is_not_changed_by_it() {
    assert_heap_guard_output(
        &with_box(
            r#"
fn bump(x Box) int
    var y = x
    y.count = 100
    return y.count

fn main()
    var b = Box(count: 1, name: "x")
    let r = bump(b)
    println(f"{r} {b.count}")
"#,
        ),
        "100 1",
    );
}

#[test]
fn test_struct_returned_from_a_function_is_written_in_place() {
    assert_heap_guard_output(
        &with_box(
            r#"
fn make() Box
    return Box(count: 1, name: "x")

fn main()
    var b = make()
    b.count = 3
    b.count += 1
    println(f"{b.count} {b.name}")
"#,
        ),
        "4 x",
    );
}

#[test]
fn test_scalar_struct_captured_by_a_closure_keeps_its_value() {
    assert_heap_guard_output(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    var p = Point(x: 1, y: 2)
    let f = fn() int: p.x
    p.x = 5
    println(f"{f()} {p.x}")
"#,
        "1 5",
    );
}

#[test]
fn test_nested_struct_write_copies_each_shared_level() {
    assert_heap_guard_output(
        &with_box(
            r#"
struct Outer
    inner Box
    tag String

fn main()
    var a = Outer(inner: Box(count: 1, name: "x"), tag: "t")
    var c = a
    let i = a.inner
    c.inner.count = 8
    a.inner.count = 7
    println(f"{a.inner.count} {c.inner.count} {i.count}")
"#,
        ),
        "7 8 1",
    );
}

#[test]
fn test_list_field_mutation_copies_a_shared_struct() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Bag
    items [int]
    name String

fn main()
    var a = Bag(items: List([1, 2]), name: "a")
    var c = a
    c.items.push(3)
    c.items[0] = 10
    a.items.push(4)
    println(f"{a.items.length()} {a.items[0]} {a.items[2]} {c.items.length()} {c.items[0]} {c.items[2]}")
"#,
        "3 1 4 3 10 3",
    );
}

#[test]
fn test_list_field_shared_with_a_binding_is_copied_before_a_push() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Bag
    items [int]
    name String

fn main()
    var a = Bag(items: List([1, 2]), name: "a")
    let held = a.items
    a.items.push(3)
    println(f"{held.length()} {a.items.length()}")
"#,
        "2 3",
    );
}

#[test]
fn test_map_field_store_copies_a_shared_struct() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

struct Index
    entries Map<String, int>
    name String

fn main()
    var a = Index(entries: {"k": 1}, name: "a")
    var c = a
    c.entries["k"] = 2
    c.entries.set("j", 3)
    println(f"{a.entries.length()} {a.entries['k']} {c.entries.length()} {c.entries['k']}")
"#,
        "1 1 2 2",
    );
}

#[test]
fn test_list_element_field_write_leaves_a_bound_element_alone() {
    assert_heap_guard_output(
        &with_box(
            r#"
use system.collections.list

fn main()
    var l = List([Box(count: 1, name: "y")])
    let s = l[0]
    l[0].count = 5
    println(f"{s.count} {l[0].count}")
"#,
        ),
        "1 5",
    );
}

#[test]
fn test_list_element_field_write_copies_a_shared_list() {
    assert_heap_guard_output(
        &with_box(
            r#"
use system.collections.list

fn main()
    var l = List([Box(count: 1, name: "y"), Box(count: 2, name: "z")])
    var m = l
    var i = 1
    m[i].count = 9
    println(f"{l[1].count} {m[1].count}")
"#,
        ),
        "2 9",
    );
}

#[test]
fn test_struct_in_a_class_field_is_copied_but_the_class_is_shared() {
    assert_heap_guard_output(
        &with_box(
            r#"
class Holder
    var b Box

fn main()
    var h = Holder(b: Box(count: 1, name: "x"))
    let other = h
    let held = h.b
    h.b.count = 6
    println(f"{held.count} {h.b.count} {other.b.count}")
"#,
        ),
        "1 6 6",
    );
}

#[test]
fn test_index_expression_of_a_written_path_runs_once() {
    assert_heap_guard_output(
        &with_box(
            r#"
use system.collections.list

class Counter
    var calls int

    fn pick() int
        self.calls += 1
        return 0

fn main()
    let counter = Counter(calls: 0)
    var l = List([Box(count: 1, name: "y")])
    l[counter.pick()].count += 4
    println(f"{l[0].count} {counter.calls}")
"#,
        ),
        "5 1",
    );
}

#[test]
fn test_map_entry_field_write_leaves_a_bound_entry_and_a_copied_map_alone() {
    assert_heap_guard_output(
        &with_box(
            r#"
use system.collections.map

fn main()
    var m Map<String, Box> = {"a": Box(count: 1, name: "x")}
    let s = m["a"]
    m["a"].count = 5
    var m2 = m
    m2["a"].count = 9
    println(f"{s.count} {m['a'].count} {m2['a'].count}")
"#,
        ),
        "1 5 9",
    );
}

#[test]
fn test_map_entry_written_under_a_computed_key_is_written_once_per_store() {
    assert_heap_guard_output(
        &with_box(
            r#"
use system.collections.map

fn main()
    var m Map<String, Box> = {"k1": Box(count: 1, name: "x")}
    let i = 1
    m[f"k{i}"].count = 4
    m[f"k{i}"].count += 1
    m[f"k{i}"].name = "y"
    println(f"{m['k1'].count} {m['k1'].name} {m.length()}")
"#,
        ),
        "5 y 1",
    );
}

#[test]
fn test_list_in_a_map_entry_is_copied_before_it_is_mutated() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

fn main()
    var m Map<String, [int]> = {"a": List([1, 2])}
    let held = m["a"]
    m["a"].push(3)
    m["a"][0] = 9
    println(f"{held.length()} {held[0]} {m['a'].length()} {m['a'][0]}")
"#,
        "2 1 3 9",
    );
}

#[test]
fn test_scalar_struct_element_of_a_copied_list_is_written_only_in_one() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Point
    x int
    y int

fn main()
    var l = List([Point(x: 1, y: 2), Point(x: 3, y: 4)])
    var l2 = l
    l[1].x = 30
    l[0].y += 5
    var a = [Point(x: 1, y: 2)]
    var a2 = a
    a2[0].x = 9
    println(f"{l[0].y} {l[1].x} {l2[0].y} {l2[1].x} {a[0].x} {a2[0].x}")
"#,
        "7 30 2 3 1 9",
    );
}
