// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Comparing an optional class inside a loop. The comparison reads each
// side's payload into a temporary; that temporary must be released when the
// comparison is done, not when the enclosing scope ends, or every turn of the
// loop leaves one reference behind.

use super::utils::*;

const NODE: &str = r#"
class Node
    var val int
    var next Node?
    fn init(v int)
        self.val = v
    fn drop(self)
        println(f"drop {self.val}")
"#;

fn with_node(program: &str) -> String {
    format!("{NODE}{program}")
}

#[test]
fn test_option_compare_none_in_while_with_if_let_walk() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?) int
    var c = 0
    var cur = head
    while cur != None
        c += 1
        if let Some(nd) = cur
            cur = nd.next
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    a.next = b
    println(f"{walk(a)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_option_compare_none_in_while_with_match_walk() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?) int
    var c = 0
    var cur = head
    while cur != None
        c += 1
        match cur
            Some(nd): cur = nd.next
            None: c += 0
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    a.next = b
    println(f"{walk(a)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_option_compare_some_in_while_releases_payloads() {
    assert_heap_guard_output(
        &with_node(
            r#"
fn walk(head Node?, stop Node?) int
    var c = 0
    var cur = head
    while cur != stop
        c += 1
        if let Some(nd) = cur
            cur = nd.next
    return c

fn main()
    var a = Node(1)
    var b = Node(2)
    var d = Node(3)
    b.next = d
    a.next = b
    println(f"{walk(a, d)}")
    println("end")
"#,
        ),
        "2\nend\ndrop 1\ndrop 2\ndrop 3",
    );
}

#[test]
fn test_option_compare_walk_returns_early() {
    assert_heap_guard_output(
        r#"
class Cell
    var val int
    var next Cell?
    fn init(v int)
        self.val = v

fn find(head Cell?, wanted int) int
    var cur = head
    while cur != None
        if let Some(cell) = cur
            if cell.val == wanted
                return cell.val
            cur = cell.next
    return -1

fn main()
    var a = Cell(1)
    var b = Cell(2)
    var d = Cell(3)
    b.next = d
    a.next = b
    println(f"{find(a, 2)}")
    println(f"{find(a, 9)}")
    println("end")
"#,
        "2\n-1\nend",
    );
}

#[test]
fn test_option_compare_walks_thousand_node_list() {
    assert_heap_guard_output(
        r#"
class Cell
    var val int
    var next Cell?
    fn init(v int)
        self.val = v

fn build(n int) Cell?
    var head Cell? = None
    for i in 0..n
        var cell = Cell(i)
        cell.next = head
        head = cell
    return head

fn walk(head Cell?) int
    var total = 0
    var cur = head
    while cur != None
        match cur
            Some(cell)
                total += cell.val
                cur = cell.next
            None: total += 0
    return total

fn main()
    let list = build(1000)
    println(f"{walk(list)}")
    println(f"{walk(list)}")
"#,
        "499500\n499500",
    );
}

// A comparison short-circuits on two values that are the same object and
// reads the members of two that are not. The member reads below happen only on
// the second path, so a loop that takes both paths releases each read on the
// pass that made it and on no other.

const PT: &str = r#"
use system.collections.list

struct Pt
    name String
    xs [int]
"#;

fn with_pt(program: &str) -> String {
    format!("{PT}{program}")
}

#[test]
fn test_optional_struct_compare_in_loop_mixes_identity_and_members() {
    assert_heap_guard_output(
        &with_pt(
            r#"
fn main()
    let b Pt? = Pt(name: f"n{1}", xs: List([1]))
    let a Pt? = Pt(name: f"n{0}", xs: List([0]))
    let c Pt? = Pt(name: f"n{1}", xs: List([1]))
    var hits = 0
    var i = 0
    while i < 6
        let p = if i % 3 == 0: a else: if i % 3 == 1: b else: c
        if p != b
            hits += 1
        if p == b
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "42",
    );
}

#[test]
fn test_struct_compare_in_loop_releases_optional_field_members() {
    assert_heap_guard_output(
        &with_pt(
            r#"
struct Wrap
    tag String
    inner Pt?

fn main()
    let same Pt? = Pt(name: f"n{1}", xs: List([1]))
    let other Pt? = Pt(name: f"n{1}", xs: List([2]))
    let w0 = Wrap(tag: f"t{0}", inner: same)
    let w1 = Wrap(tag: f"t{0}", inner: same)
    let w2 = Wrap(tag: f"t{0}", inner: other)
    var hits = 0
    var i = 0
    while i < 6
        let w = if i % 2 == 0: w1 else: w2
        if w0 == w
            hits += 1
        if w0 != w
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "33",
    );
}

#[test]
fn test_tuple_compare_in_loop_releases_optional_element_members() {
    assert_heap_guard_output(
        &with_pt(
            r#"
fn main()
    let same Pt? = Pt(name: f"n{1}", xs: List([1]))
    let other Pt? = Pt(name: f"n{1}", xs: List([1]))
    let t0 = (f"k{0}", same)
    let t1 = (f"k{0}", same)
    let t2 = (f"k{0}", other)
    var hits = 0
    var i = 0
    while i < 6
        let t = if i % 2 == 0: t1 else: t2
        if t0 == t
            hits += 1
        if t0 != t
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "6",
    );
}

#[test]
fn test_enum_compare_in_loop_releases_payload_struct_members() {
    assert_heap_guard_output(
        &with_pt(
            r#"
enum Shape
    Dot(Pt?)
    Empty

fn main()
    let same Pt? = Pt(name: f"n{1}", xs: List([1]))
    let other Pt? = Pt(name: f"n{2}", xs: List([1]))
    let e0 = Shape.Dot(same)
    let e1 = Shape.Dot(same)
    let e2 = Shape.Dot(other)
    var hits = 0
    var i = 0
    while i < 6
        let e = if i % 2 == 0: e1 else: e2
        if e0 == e
            hits += 1
        if e0 != e
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "33",
    );
}

#[test]
fn test_nested_optional_struct_compare_in_loop() {
    assert_heap_guard_output(
        &with_pt(
            r#"
struct Holder
    label String
    pt Pt?

fn main()
    let same Pt? = Pt(name: f"n{1}", xs: List([1]))
    let other Pt? = Pt(name: f"n{1}", xs: List([1]))
    let x Holder? = Holder(label: f"l{0}", pt: same)
    let y Holder? = Holder(label: f"l{0}", pt: same)
    let z Holder? = Holder(label: f"l{0}", pt: other)
    var hits = 0
    var i = 0
    while i < 6
        let o = if i % 3 == 0: x else: if i % 3 == 1: y else: z
        if x == o
            hits += 1
        if x != o
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "6",
    );
}

#[test]
fn test_map_lookup_compare_in_loop_releases_struct_members() {
    assert_heap_guard_output(
        &with_pt(
            r#"
use system.collections.map

fn main()
    var mp = Map<String, Pt>()
    mp["s0"] = Pt(name: "n0", xs: List([0]))
    mp["s1"] = Pt(name: "n1", xs: List([1]))
    mp["s2"] = Pt(name: "n0", xs: List([0]))
    var hits = 0
    var i = 0
    while i < 6
        let k = f"s{i % 3}"
        if mp.get(k) != mp.get("s0")
            hits += 1
        if mp.get(k) == mp.get(k)
            hits += 10
        i += 1
    println(f"{hits}")
"#,
        ),
        "62",
    );
}
