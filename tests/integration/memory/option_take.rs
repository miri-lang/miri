// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `take()` on an optional hands its value to the caller, owned, and leaves
//! `None` in the place it was called on: a local, a field, a list element or
//! an `out` parameter. It is `replace` on the optional's own receiver, so the
//! value leaves the place exactly once: a counted payload neither leaks nor is
//! freed twice, and a drop hook runs once, where the taken value ends.

use super::super::utils::*;

const CONN: &str = r#"
use system.io

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")

fn show(c Conn?) String
    match c
        Some(conn): return f"conn {conn.id}"
        None: return "none"
"#;

fn with_conn(program: &str) -> String {
    format!("{CONN}\n{program}")
}

#[test]
fn take_on_a_local_hands_back_the_value_and_leaves_none() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var slot Conn? = Some(Conn(id: 1))
    let taken = slot.take()
    println(f"{show(taken)} {show(slot)}")
"#,
        ),
        "conn 1 none\nclosed 1",
    );
}

#[test]
fn take_on_a_field_leaves_none_in_the_field() {
    assert_heap_guard_output(
        &with_conn(
            r#"
class Holder
    var conn Conn?

fn main()
    var h = Holder(conn: Some(Conn(id: 2)))
    let taken = h.conn.take()
    println(f"{show(taken)} {show(h.conn)}")
"#,
        ),
        "conn 2 none\nclosed 2",
    );
}

#[test]
fn take_on_a_list_element_leaves_none_in_the_list() {
    assert_heap_guard_output(
        &with_conn(
            r#"
use system.collections.list

fn main()
    var slots = List<Conn?>()
    slots.push(Some(Conn(id: 3)))
    slots.push(Some(Conn(id: 4)))
    let taken = slots[0].take()
    println(f"{show(taken)} {show(slots[0])} {show(slots[1])}")
"#,
        ),
        "conn 3 none conn 4\nclosed 3\nclosed 4",
    );
}

#[test]
fn take_on_an_out_parameter_leaves_none_in_the_callers_place() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn steal(slot out Conn?) Conn?
    return slot.take()

fn main()
    var slot Conn? = Some(Conn(id: 5))
    let taken = steal(slot)
    println(f"{show(taken)} {show(slot)}")
"#,
        ),
        "conn 5 none\nclosed 5",
    );
}

#[test]
fn take_runs_the_hook_where_the_taken_value_ends() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var slot Conn? = Some(Conn(id: 6))
    if true
        let taken = slot.take()
        println("taken")
    println("after")
"#,
        ),
        "taken\nclosed 6\nafter",
    );
}

#[test]
fn take_on_an_empty_optional_hands_back_none() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var slot Conn? = None
    let taken = slot.take()
    println(f"{show(taken)} {show(slot)}")
"#,
        ),
        "none none",
    );
}

#[test]
fn take_hands_back_a_string_payload_without_a_leak() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var name String? = Some("payload")
    let taken = name.take()
    println(f"{taken ?? '-'} {name ?? '-'}")
"#,
        "payload -",
    );
}

#[test]
fn a_sibling_operand_sees_an_optional_int_before_take_empties_it() {
    assert_heap_guard_output(
        r#"
use system.io

fn both(a int?, b int?) String
    return f"{a.is_some()} {b.is_some()}"

fn main()
    var q int? = Some(1)
    println(both(q, q.take()))
    println(f"{q.is_none()}")
"#,
        "true true\ntrue",
    );
}

#[test]
fn a_sibling_operand_sees_an_optional_string_before_take_empties_it() {
    assert_heap_guard_output(
        r#"
use system.io

fn both(a String?, b String?) String
    return f"{a ?? '-'} {b ?? '-'}"

fn main()
    var q String? = Some("q")
    println(both(q, q.take()))
    println(f"{q ?? '-'}")
"#,
        "q q\n-",
    );
}

#[test]
fn a_sibling_operand_sees_an_optional_resource_before_take_empties_it() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn both(a Conn?, b Conn?) String
    return f"{show(a)} {show(b)}"

fn main()
    var q Conn? = Some(Conn(id: 4))
    println(both(q, q.take()))
    println(show(q))
"#,
        ),
        "closed 4\nconn 4 conn 4\nnone",
    );
}

#[test]
fn take_on_an_optional_of_an_optional_hands_back_the_outer_value() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var o Option<Conn?> = Some(Some(Conn(id: 7)))
    let t = o.take()
    match t
        Some(x): println("outer some")
        None: println("outer none")
    match o
        Some(x): println("still")
        None: println("none")
"#,
        ),
        "outer some\nnone\nclosed 7",
    );
}

#[test]
fn take_twice_hands_back_none_the_second_time() {
    assert_heap_guard_output(
        &with_conn(
            r#"
fn main()
    var s Conn? = Some(Conn(id: 1))
    let a = s.take()
    let b = s.take()
    println(f"{show(a)} {show(b)} {show(s)}")
"#,
        ),
        "conn 1 none none\nclosed 1",
    );
}

#[test]
fn take_in_a_loop_hands_back_each_value_the_loop_refills() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var i = 0
    var t String? = Some("p")
    while i < 3
        let q = t.take()
        println(f"{q ?? '-'}")
        t = Some("again")
        i += 1
"#,
        "p\nagain\nagain",
    );
}

#[test]
fn take_on_a_list_element_inside_a_loop_over_indices() {
    assert_heap_guard_output(
        &with_conn(
            r#"
use system.collections.list

fn main()
    var xs = List<Conn?>()
    xs.push(Some(Conn(id: 1)))
    for i in 0..1
        let t = xs[i].take()
        println(f"loop {show(t)}")
    println(f"end {show(xs[0])}")
"#,
        ),
        "loop conn 1\nclosed 1\nend none",
    );
}

#[test]
fn take_on_a_field_of_self_inside_a_class_method() {
    assert_heap_guard_output(
        r#"
use system.io

class W
    var s String?

    fn grab() String?
        return self.s.take()

fn main()
    var w = W(s: Some("hi"))
    let a = w.grab()
    println(f"{a ?? '-'} {w.s ?? '-'}")
"#,
        "hi -",
    );
}

#[test]
fn take_inside_a_generic_function_empties_its_out_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

fn refill<T>(a out T?, b T) T?
    let x = a.take()
    a = Some(b)
    return x

fn main()
    var s String? = Some("a")
    let r = refill(s, "b")
    println(f"{r ?? '-'} {s ?? '-'}")
"#,
        "a b",
    );
}

#[test]
fn take_on_a_let_binding_empties_it() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let s String? = Some("a")
    let t = s.take()
    println(f"{t ?? '-'} {s ?? '-'}")
    let u = s
    let w = u.take()
    println(f"{w ?? '-'} {u ?? '-'} {s ?? '-'}")
"#,
        "a -\n- - -",
    );
}
