// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional class or string, held as its payload's own pointer, flowing
//! through the places a program keeps values: early exits from a `Some` arm,
//! enum payloads, closure captures, tuple and struct copies, map entries,
//! generic fields, list removal, ordering, long chains, aliases, method
//! parameters, equality, interpolation and sets. Every case runs under the
//! heap guard, so a box opened that is not there, or a payload released twice,
//! fails the test rather than printing the right bytes by luck.

use super::utils::*;

#[test]
fn test_an_early_return_and_a_break_inside_a_some_arm_release_what_they_hold() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var value int
    var next Node?

fn first_even(n Node?) int
    match n
        Some(x):
            if x.value % 2 == 0
                return x.value
            return first_even(x.next)
        None: return -1

fn count(head Node?) int
    var cur = head
    var total = 0
    while true
        match cur
            Some(x):
                total = total + 1
                cur = x.next
            None: break
    total

fn main()
    let list Node? = Some(Node(value: 1, next: Some(Node(value: 2, next: Some(Node(value: 3, next: None))))))
    println(f"{first_even(list)}")
    println(f"{count(list)}")
"#,
        "2\n3",
    );
}

#[test]
fn test_an_enum_payload_holding_an_optional_class_is_matched_and_released() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

enum Slot
    Link(Node?)
    Empty

fn main()
    var slots = List<Slot>()
    slots.push(Slot.Link(Some(Node(name: "a" + "1"))))
    slots.push(Slot.Link(None))
    slots.push(Slot.Empty)
    for s in slots
        match s
            Slot.Link(Some(n)): println(f"link {n.name}")
            Slot.Link(None): println("link none")
            Slot.Empty: println("empty")
    println("end")
"#,
        "link a1\nlink none\nempty\nend\ndrop a1",
    );
}

#[test]
fn test_a_closure_capturing_an_optional_class_reads_some_and_none() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var value int

fn make(o Node?) fn() int
    fn() int
        match o
            Some(n): n.value
            None: -1

fn main()
    let f = make(Some(Node(value: 8)))
    let g = make(None)
    println(f"{f()} {g()}")
"#,
        "8 -1",
    );
}

#[test]
fn test_copying_a_tuple_of_optional_classes_keeps_both_copies() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

class Holder
    var n Node?

fn show(o Node?) String
    match o
        Some(n): n.name
        None: "none"

fn main()
    let t (Node?, Node?) = (Some(Node(name: "a" + "1")), None)
    var u = t
    println(f"{show(t.0)} {show(t.1)} {show(u.0)}")
    let h = Holder(n: Some(Node(name: "b" + "2")))
    var copy = Holder(n: h.n)
    copy.n = None
    println(f"{show(h.n)} {show(copy.n)}")
"#,
        "a1 none a1\nb2 none\ndrop b2\ndrop a1",
    );
}

#[test]
fn test_setting_a_struct_copy_optional_field_to_none_leaves_the_original() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

struct Cell
    n Node?
    k int

fn show(o Node?) String
    match o
        Some(n): n.name
        None: "none"

fn main()
    let s = Cell(n: Some(Node(name: "a" + "1")), k: 1)
    var c = s
    c.n = None
    println(f"{show(s.n)} {show(c.n)}")
"#,
        "a1 none\ndrop a1",
    );
}

#[test]
fn test_a_map_of_optional_classes_tells_an_inner_none_from_an_absent_key() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

class Node
    var name String

fn main()
    var m = Map<String, Node?>()
    m["some"] = Some(Node(name: "x" + "1"))
    m["none"] = None
    for key in ["some", "none", "gone"]
        match m.get(key)
            Some(inner):
                match inner
                    Some(n): println(f"{key} {n.name}")
                    None: println(f"{key} inner none")
            None: println(f"{key} absent")
"#,
        "some x1\nnone inner none\ngone absent",
    );
}

#[test]
fn test_a_generic_field_of_an_optional_parameter_at_an_optional_class_boxes_the_outer_layer() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

class Holder<T>
    var v T?

fn main()
    let h = Holder<Node?>(v: Some(Some(Node(name: "a" + "1"))))
    match h.v
        Some(inner):
            match inner
                Some(n): println(n.name)
                None: println("inner none")
        None: println("none")
    let e = Holder<Node?>(v: Some(None))
    match e.v
        Some(inner):
            match inner
                Some(n): println(n.name)
                None: println("inner none")
        None: println("none")
    let z = Holder<Node>(v: Some(Node(name: "b" + "2")))
    match z.v
        Some(n): println(n.name)
        None: println("none")
"#,
        "a1\ninner none\nb2\ndrop b2\ndrop a1",
    );
}

#[test]
fn test_remove_at_on_a_list_of_optional_classes_hands_back_each_layer() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

fn show(r Option<Node?>) String
    match r
        Some(Some(n)): f"removed {n.name}"
        Some(None): "removed none"
        None: "out of range"

fn main()
    var xs = List<Node?>()
    xs.push(Some(Node(name: "a" + "1")))
    xs.push(None)
    xs.push(Some(Node(name: "b" + "2")))
    println(show(xs.remove_at(0)))
    println(show(xs.remove_at(0)))
    println(show(xs.remove_at(5)))
    println(f"{xs.length()}")
"#,
        "drop a1\nremoved a1\nremoved none\nout of range\n1\ndrop b2",
    );
}

#[test]
fn test_min_and_max_of_a_comparable_class_are_optional_pointers() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node implements Comparable
    var v int
    fn compare(other Node) int
        self.v - other.v

fn show(o Node?) String
    match o
        Some(n): f"{n.v}"
        None: "none"

fn main()
    var xs = List<Node>()
    xs.push(Node(v: 5))
    xs.push(Node(v: 2))
    xs.push(Node(v: 9))
    println(f"{show(xs.min())} {show(xs.max())}")
    let empty = List<Node>()
    println(f"{show(empty.min())} {show(empty.max())}")
"#,
        "2 9\nnone none",
    );
}

#[test]
fn test_a_long_linked_list_of_optional_classes_is_built_walked_and_released() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var value int
    var next Node?

fn main()
    var head Node? = None
    var i = 0
    while i < 100000
        head = Some(Node(value: i, next: head))
        i += 1
    var total = 0
    var cur = head
    while true
        match cur
            Some(n):
                total = total + 1
                cur = n.next
            None: break
    println(f"{total}")
"#,
        "100000",
    );
}

#[test]
fn test_reassigning_an_aliased_optional_class_drops_it_once_at_scope_end() {
    assert_heap_guard_output(
        r#"
use system.io

class Res
    var id int
    fn drop()
        println(f"drop {self.id}")

fn main()
    var r Res? = Some(Res(id: 1))
    let _again = r
    r = r
    r = None
    println("mid")
"#,
        "mid\ndrop 1",
    );
}

#[test]
fn test_an_optional_string_stored_through_a_method_parameter_keeps_its_value() {
    assert_heap_guard_output(
        r#"
use system.io

class Box
    var s String?
    fn set(x String?)
        self.s = x
    fn show() String
        match self.s
            Some(v): v
            None: "none"

fn main()
    var b = Box(s: None)
    b.set(Some("rt"))
    let first = b.show()
    b.set(Some("r" + "t"))
    println(f"{first} {b.show()}")
"#,
        "rt rt",
    );
}

#[test]
fn test_optional_classes_compare_by_identity_through_every_layer() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var v int

fn main()
    let a = Node(v: 1)
    let x Node? = Some(a)
    let y Node? = Some(a)
    println(f"{x == y}")
    let n Option<Node?> = Some(Some(a))
    let m Option<Node?> = Some(Some(a))
    println(f"{n == m}")
    let none Node? = None
    println(f"{x == none}")
    let some_none Option<Node?> = Some(None)
    let bare Option<Node?> = None
    println(f"{some_none == bare}")
"#,
        "true\ntrue\nfalse\nfalse",
    );
}

#[test]
fn test_optional_strings_render_their_layers_in_f_strings() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var v int

fn main()

    let sn Option<String?> = Some(None)
    let s String? = Some("h" + "i")
    let none String? = None
    println(f"{sn} | {s} | {none}")
"#,
        "Some(None) | Some(hi) | None",
    );
}

#[test]
fn test_a_set_of_optional_strings_removes_none_and_some_by_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = Set<String?>()
    s.add(Some("a" + "b"))
    s.add(None)
    s.add(Some("cd"))
    println(f"{s.length()}")
    s.remove(None)
    println(f"{s.length()}")
    s.remove(Some("ab"))
    println(f"{s.length()}")
    let probe String? = Some("c" + "d")
    println(f"{s.contains(probe)}")
"#,
        "3\n2\n1\ntrue",
    );
}
