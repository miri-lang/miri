// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A loop over a list may call functions of the program and still read the
//! list's header once, before the loop, when no call names the list or
//! anything that reaches it without a reference of its own. Each program here
//! calls a function that writes a list the loop reads, or a holder of it, and
//! must print what it printed before that header was read once: a write
//! through a holder with a reference of its own copies the list first, so the
//! loop's list keeps its header.
//!
//! `tests/mir/optimization/stable_collections.rs` checks which of these loops
//! are recorded; every program also runs clean under the heap guard.

use super::super::utils::*;

/// A callee handed the loop's list twice writes one of its parameters with
/// the writer `{writer}`.
pub(crate) fn callee_writing_its_parameter(writer: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list

fn grow(x [int], y [int]) int
    x.{writer}
    x.length() + y.length()

fn main()
    var a = List([1, 2, 3])
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + grow(a, a)
        i += 1
    println(f"{{total}} {{a.length()}}")
"#
    )
}

pub(crate) const THROUGH_A_CLASS_FIELD: &str = r#"
use system.io
use system.collections.list

class Holder
    var items [int]

fn fill(h Holder) int
    h.items.push(5)
    h.items.length()

fn main()
    var a = List([1, 2, 3])
    let h = Holder(items: a)
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + fill(h)
        i += 1
    println(f"{total} {a.length()} {h.items.length()}")
"#;

pub(crate) const THROUGH_A_PASSED_CLOSURE: &str = r#"
use system.io
use system.collections.list

fn run(f fn(int) int, v int) int
    f(v)

fn main()
    var a = List([1, 2, 3])
    let grow = fn(v int) int
        a.clear()
        a.length() + v
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + run(grow, i)
        i += 1
    println(f"{total} {a.length()}")
"#;

pub(crate) const THROUGH_AN_ELEMENT_POINTING_BACK: &str = r#"
use system.io
use system.collections.list

class Node
    var back [int]

fn poke(nodes [Node]) int
    nodes[0].back.clear()
    nodes[0].back.length()

fn main()
    var a = List([1, 2, 3])
    let nodes = List([Node(back: a)])
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + poke(nodes)
        i += 1
    println(f"{total} {a.length()} {nodes[0].back.length()}")
"#;

pub(crate) const THROUGH_ANOTHER_BINDING: &str = r#"
use system.io
use system.collections.list

fn extend(x [int]) int
    x.push(7)
    x.length()

fn main()
    var a = List([1, 2, 3])
    var b = a
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + extend(b)
        i += 1
    println(f"{total} {a.length()} {b.length()}")
"#;

pub(crate) const CLOSURE_PUSHING_ITS_CAPTURE: &str = r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    let add = fn(v int) int
        a.push(v)
        a.length()
    var i = 0
    var total = 0
    while i < a.length() and i < 5
        total = total + a[i] + add(i)
        i += 1
    println(f"{total} {a.length()}")
"#;

pub(crate) const RECURSIVE_CALLEE: &str = r#"
use system.io
use system.collections.list

fn walk(xs [int], depth int) int
    if depth == 0
        return 0
    var i = 0
    var total = 0
    while i < xs.length()
        total = total + xs[i] + walk(xs, depth - 1)
        i += 1
    total

fn main()
    let a = List([1, 2, 3])
    println(f"{walk(a, 3)} {a.length()}")
"#;

pub(crate) const LOOP_OVER_A_FIELD: &str = r#"
use system.io
use system.collections.list

class Holder
    var items [int]

fn fill(h Holder) int
    h.items.push(5)
    h.items.length()

fn main()
    let h = Holder(items: List([1, 2, 3]))
    var total = 0
    for v in h.items
        total = total + v + fill(h)
    println(f"{total} {h.items.length()}")
"#;

pub(crate) const OWNED_PARAMETER_TRANSFER: &str = r#"
use system.io
use system.collections.list

class Holder
    var items [int]

fn wrap(xs [int]) Holder
    Holder(items: xs)

fn main()
    var a = List([1, 2, 3])
    var i = 0
    var total = 0
    while i < a.length()
        let h = wrap(a)
        h.items.push(i)
        total = total + a[i] + h.items.length()
        i += 1
    println(f"{total} {a.length()}")
"#;

pub(crate) const OUT_ARGUMENT_ALIAS: &str = r#"
use system.io
use system.collections.list

fn refill(xs out [int])
    xs = List([7, 8])

fn main()
    var a = List([1, 2, 3])
    var b = a
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i]
        refill(b)
        i += 1
    println(f"{total} {a.length()} {b.length()}")
"#;

pub(crate) const VIRTUAL_CALL: &str = r#"
use system.io
use system.collections.list

trait Sized
    fn size() int

class Box implements Sized
    var n int
    fn size() int
        return self.n

fn main()
    let s Sized = Box(n: 2)
    let a = List([1, 2, 3])
    var total = 0
    for v in a
        total = total + v * s.size()
    println(f"{total}")
"#;

pub(crate) const INDEX_WRITTEN_THROUGH_OUT: &str = r#"
use system.io
use system.collections.list

fn bump(i out int)
    i = i + 1

fn main()
    let a = List([1, 2, 3])
    var i = 0
    var total = 0
    while i < a.length()
        bump(i)
        total = total + a[i]
    println(f"{total}")
"#;

pub(crate) const BORROWED_PARAMETER_COPIED_IN_THE_LOOP: &str = r#"
use system.io
use system.collections.list

fn touch(xs [int]) int
    xs.push(1)
    xs.length()

fn sum(xs [int]) int
    let a = List([1, 2, 3])
    var total = 0
    for v in a
        total = total + v + touch(xs)
    total + xs.length()

fn main()
    let xs = List([4])
    println(f"{sum(xs)} {xs.length()}")
"#;

pub(crate) const SPHERES: &str = r#"
use system.io
use system.collections.list

struct Vec3
    x float
    y float
    z float

struct Sphere
    center Vec3
    radius float

fn dot(a Vec3, b Vec3) float
    a.x * b.x + a.y * b.y + a.z * b.z

fn hit(s Sphere, d Vec3) float
    let b = dot(s.center, d)
    if b > s.radius: b else: 0.0

fn shade(spheres [Sphere], d Vec3, rounds int) float
    var total = 0.0
    var n = 0
    while n < rounds
        for sphere in spheres
            total = total + hit(sphere, d)
        n += 1
    total

fn main()
    let near = Sphere(center: Vec3(x: 0.0, y: 0.0, z: 3.0), radius: 1.0)
    let far = Sphere(center: Vec3(x: 1.0, y: 0.0, z: 5.0), radius: 2.0)
    let spheres = List([near, far])
    println(f"{shade(spheres, Vec3(x: 0.0, y: 0.0, z: 1.0), 3)}")
"#;

pub(crate) const ALIAS_BOUND_IN_THE_LOOP: &str = r#"
use system.io
use system.collections.list

fn wipe(x [int]) int
    x.clear()
    x.length()

fn main()
    var c = List([1, 2, 3])
    var i = 0
    var total = 0
    while i < c.length()
        let d = c
        total = total + c[i] + wipe(d)
        i += 1
    println(f"{total} {c.length()}")
"#;

pub(crate) const WRITER_THROUGH_A_COUNTED_FIELD: &str = r#"
use system.io
use system.collections.list

class Bag
    var items [int]
    fn reset()
        self.items.clear()
        self.items.reserve(8)

fn main()
    var a = List([1, 2, 3])
    let bag = Bag(items: a)
    var total = 0
    for v in a
        bag.reset()
        total = total + v + bag.items.length()
    println(f"{total} {a.length()} {bag.items.length()}")
"#;

pub(crate) const CLOSURE_IN_A_FIELD: &str = r#"
use system.io
use system.collections.list

class Hook
    run fn(int) int

fn main()
    var a = List([1, 2, 3])
    let wipe = fn(v int) int
        a.clear()
        a.length() + v
    let hook = Hook(run: wipe)
    var i = 0
    var total = 0
    while i < a.length()
        total = total + a[i] + hook.run(i)
        i += 1
    println(f"{total} {a.length()}")
"#;

pub(crate) const LIST_FROM_AN_ELEMENT_ACCESSOR: &str = r#"
use system.io
use system.collections.list

class Grid
    var rows [[int]]
    fn widen() int
        self.rows[0].push(9)
        self.rows[0].length()

fn main()
    let g = Grid(rows: List([List([1, 2, 3])]))
    let a = g.rows.get(0)
    var total = 0
    for v in a
        total = total + v + g.widen()
    println(f"{total} {a.length()} {g.rows[0].length()}")
"#;

pub(crate) const LIST_FROM_A_MAP_VALUE_ACCESSOR: &str = r#"
use system.io
use system.collections.list
use system.collections.map

class Table
    var lists {int: [int]}
    fn widen() int
        var row = self.lists.value_at(0)
        row.push(9)
        self.lists[1] = row
        row.length()

fn main()
    let t = Table(lists: {1: List([1, 2, 3])})
    let a = t.lists.value_at(0)
    var total = 0
    for v in a
        total = total + v + t.widen()
    println(f"{total} {a.length()} {t.lists.value_at(0).length()}")
"#;

pub(crate) const LIST_FROM_A_METHOD_RETURNING_A_FIELD: &str = r#"
use system.io
use system.collections.list

class Bag
    var items [int]
    fn snapshot() [int]
        self.items
    fn add(v int) int
        self.items.push(v)
        self.items.length()

fn main()
    let bag = Bag(items: List([1, 2, 3]))
    let a = bag.snapshot()
    var total = 0
    for v in a
        total = total + v + bag.add(v)
    println(f"{total} {a.length()} {bag.items.length()}")
"#;

#[test]
fn test_a_callee_reserving_into_the_loop_list_writes_its_own_copy() {
    assert_heap_guard_output(&callee_writing_its_parameter("reserve(100)"), "24 3");
}

#[test]
fn test_a_callee_resizing_the_loop_list_writes_its_own_copy() {
    assert_heap_guard_output(&callee_writing_its_parameter("resize(10, 0)"), "45 3");
}

#[test]
fn test_a_callee_popping_the_loop_list_writes_its_own_copy() {
    assert_heap_guard_output(&callee_writing_its_parameter("pop()"), "21 3");
}

#[test]
fn test_a_callee_pushing_onto_the_loop_list_writes_its_own_copy() {
    assert_heap_guard_output(&callee_writing_its_parameter("push(9)"), "27 3");
}

#[test]
fn test_a_callee_clearing_the_loop_list_writes_its_own_copy() {
    assert_heap_guard_output(&callee_writing_its_parameter("clear()"), "15 3");
}

#[test]
fn test_a_callee_writing_the_loop_list_through_a_class_field_copies_it() {
    assert_heap_guard_output(THROUGH_A_CLASS_FIELD, "21 3 6");
}

#[test]
fn test_a_closure_capturing_the_loop_list_passed_to_a_callee_clears_its_copy() {
    assert_heap_guard_output(THROUGH_A_PASSED_CLOSURE, "9 3");
}

#[test]
fn test_a_callee_clearing_the_loop_list_through_an_element_copies_it() {
    assert_heap_guard_output(THROUGH_AN_ELEMENT_POINTING_BACK, "6 3 0");
}

#[test]
fn test_a_callee_pushing_through_another_binding_leaves_the_loop_list() {
    assert_heap_guard_output(THROUGH_ANOTHER_BINDING, "18 3 3");
}

#[test]
fn test_a_closure_pushing_onto_its_captured_loop_list_leaves_the_loop_list() {
    assert_heap_guard_output(CLOSURE_PUSHING_ITS_CAPTURE, "18 3");
}

#[test]
fn test_a_recursive_callee_handed_the_loop_list_leaves_it() {
    assert_heap_guard_output(RECURSIVE_CALLEE, "78 3");
}

#[test]
fn test_a_loop_over_a_field_whose_holder_a_callee_pushes_onto_keeps_its_length() {
    assert_heap_guard_output(LOOP_OVER_A_FIELD, "21 6");
}

#[test]
fn test_a_callee_taking_over_the_loop_list_writes_a_copy() {
    assert_heap_guard_output(OWNED_PARAMETER_TRANSFER, "18 3");
}

#[test]
fn test_an_out_argument_sharing_the_loop_list_replaces_only_its_binding() {
    assert_heap_guard_output(OUT_ARGUMENT_ALIAS, "6 3 2");
}

#[test]
fn test_a_virtual_call_in_the_loop_reads_the_list_unchanged() {
    assert_heap_guard_output(VIRTUAL_CALL, "12");
}

#[test]
fn test_an_index_a_callee_moves_past_the_end_still_traps() {
    assert_runtime_error(
        INDEX_WRITTEN_THROUGH_OUT,
        "Array index out of bounds: the len is 3 but the index is 3",
    );
}

#[test]
fn test_a_borrowed_parameter_a_callee_writes_in_a_loop_keeps_its_value() {
    assert_heap_guard_output(BORROWED_PARAMETER_COPIED_IN_THE_LOOP, "13 1");
}

#[test]
fn test_a_loop_over_spheres_calling_a_hit_test_sums_every_hit() {
    assert_heap_guard_output(SPHERES, "24.0");
}

#[test]
fn test_an_alias_bound_in_the_loop_and_cleared_by_a_callee_leaves_the_loop_list() {
    assert_heap_guard_output(ALIAS_BOUND_IN_THE_LOOP, "6 3");
}

#[test]
fn test_a_method_clearing_a_field_sharing_the_loop_list_copies_it() {
    assert_heap_guard_output(WRITER_THROUGH_A_COUNTED_FIELD, "6 3 0");
}

#[test]
fn test_a_closure_in_a_field_clearing_its_capture_leaves_the_loop_list() {
    assert_heap_guard_output(CLOSURE_IN_A_FIELD, "9 3");
}

#[test]
fn test_a_list_read_off_an_element_accessor_keeps_its_length_while_its_holder_grows() {
    assert_heap_guard_output(LIST_FROM_AN_ELEMENT_ACCESSOR, "21 3 6");
}

#[test]
fn test_a_list_read_off_a_map_value_accessor_keeps_its_length_while_its_holder_grows() {
    assert_heap_guard_output(LIST_FROM_A_MAP_VALUE_ACCESSOR, "21 3 6");
}

#[test]
fn test_a_list_a_method_returns_from_a_field_keeps_its_length_while_the_field_grows() {
    assert_heap_guard_output(LIST_FROM_A_METHOD_RETURNING_A_FIELD, "21 3 6");
}
