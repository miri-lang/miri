// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for class instances holding managed fields, class instances stored in
// collections, and deeply chained object graphs.

use super::super::utils::*;

#[test]
fn test_class_with_list_and_another_list_fields_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Report
    var tags [String]
    var scores [int]
    var total int

fn main()
    let r = Report(tags: List(["a", "b", "c"]), scores: List([90, 85]), total: 175)
    println(f"{r.tags.length()}")
    println(f"{r.scores.length()}")
    println(f"{r.total}")
"#,
        "3\n2\n175",
    );
}

#[test]
fn test_class_with_set_field_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

class Unique
    var items {int}

fn main()
    let u = Unique(items: {1, 2, 3, 4})
    println(f"{u.items.length()}")
"#,
        "4",
    );
}

#[test]
fn test_list_of_class_instances_no_leak() {
    // Each class instance is heap-allocated; List holds an alias (RC bump).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Point
    var x int
    var y int

fn main()
    let pts = List([Point(x: 1, y: 2), Point(x: 3, y: 4), Point(x: 5, y: 6)])
    println(f"{pts.length()}")
"#,
        "3",
    );
}

#[test]
fn test_list_of_classes_aliased_element_outlives_list() {
    // Pull an element out of the list before dropping the list;
    // element must survive because its RC is still > 0.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Item
    var value int

fn main()
    var items = List([Item(value: 42), Item(value: 7)])
    let kept = items.element_at(0)  // IncRef kept item
    items = List([])                // old list and its two elements DecRef'd
                                    // kept still has RC = 1
    println(f"{kept.value}")
"#,
        "42",
    );
}

#[test]
fn test_map_of_class_instances_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Config
    var enabled int
    var level int

fn main()
    let cfg = {"debug": Config(enabled: 1, level: 3), "prod": Config(enabled: 0, level: 1)}
    println(f"{cfg.length()}")
"#,
        "2",
    );
}

#[test]
fn test_four_level_class_chain_no_leak() {
    // A → B → C → D: each holds a managed reference to the next.
    assert_runs_with_output(
        r#"
use system.io

class D
    var value int

class C
    var d D

class B
    var c C

class A
    var b B

fn main()
    let a = A(b: B(c: C(d: D(value: 99))))
    println(f"{a.b.c.d.value}")
"#,
        "99",
    );
}

#[test]
fn test_class_chain_in_function_scope_no_leak() {
    // Same deep chain but allocated inside a helper; must be fully freed on return.
    assert_runs_with_output(
        r#"
use system.io

class D
    var v int

class C
    var d D

class B
    var c C

class A
    var b B

fn deep_value() int
    let a = A(b: B(c: C(d: D(v: 7))))
    a.b.c.d.v

fn main()
    println(f"{deep_value()}")
"#,
        "7",
    );
}

#[test]
fn test_class_holding_list_aliased_externally_no_leak() {
    // The List is created externally (RC=1), stored in a class (RC=2).
    // After the class drops, the external binding still holds a live reference.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Wrapper
    var data [int]

fn main()
    let base_list = List([10, 20, 30])
    let w = Wrapper(data: base_list)   // RC(base_list) = 2
    println(f"{w.data.length()}")      // access via class field
    println(f"{base_list.length()}")   // original binding still valid
"#,
        "3\n3",
    );
}

#[test]
fn test_class_list_field_mutation_no_leak() {
    // Appending to a List field; the List is shared, push is in-place.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Bag
    var items [int]

fn main()
    var bag = Bag(items: List([1, 2]))
    bag.items.push(3)
    bag.items.push(4)
    println(f"{bag.items.length()}")
"#,
        "4",
    );
}

#[test]
fn test_replacing_class_list_field_drops_old_list() {
    // Assign a brand-new List to a class field; old List must be DecRef'd.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Holder
    var data [int]

fn main()
    var h = Holder(data: List([1, 2, 3, 4, 5]))
    h.data = List([99])
    println(f"{h.data.length()}")
"#,
        "1",
    );
}

#[test]
fn test_class_instance_passed_through_chain_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Packet
    var payload [int]

fn step1(p Packet) int
    p.payload.length()

fn step2(p Packet) int
    step1(p) + 1

fn step3(p Packet) int
    step2(p) + 1

fn main()
    let pkt = Packet(payload: List([1, 2, 3]))
    println(f"{step3(pkt)}")
"#,
        "5",
    );
}

// ── Custom elem_drop_fn — collection mutation ops (task 2.4b) ─────────────────

#[test]
fn test_set_of_class_instances_clear_no_crash() {
    // Set<Widget>: __decref_Widget must be set as elem_drop_fn so that clear()
    // properly DecRefs each instance.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
use system.collections.list

class Widget
    var id int

fn main()
    var ws = {Widget(id: 1), Widget(id: 2), Widget(id: 3)}
    ws.clear()
    println(f"{ws.length()}")
"#,
        "0",
    );
}

#[test]
fn test_map_value_custom_remove_no_crash() {
    // Map<String, Config>: __decref_Config must be set as val_drop_fn so that
    // remove() properly DecRefs the removed value.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Config
    var level int

fn main()
    var m = {"a": Config(level: 1), "b": Config(level: 2)}
    m.remove("a")
    println(f"{m.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_value_custom_clear_no_crash() {
    // Map<String, Config>: clear() must DecRef all values via val_drop_fn.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

class Config
    var level int

fn main()
    var m = {"x": Config(level: 10), "y": Config(level: 20)}
    m.clear()
    println(f"{m.length()}")
"#,
        "0",
    );
}
