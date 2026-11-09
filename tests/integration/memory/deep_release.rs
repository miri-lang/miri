// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Releasing a long chain of objects uses a bounded amount of stack: a chain's
//! length is data, and the program that builds it has no recursion of its own
//! that could overflow.

use super::super::utils::*;

#[test]
fn test_a_two_million_node_list_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var next Node?
    var v int
    fn init(v int)
        self.v = v

fn main()
    var head = Node(0)
    for i in 1..2000000
        var n = Node(i)
        n.next = head
        head = n
    println(f"{head.v}")
"#,
        "1999999",
    );
}

#[test]
fn test_drop_hooks_run_in_chain_order_on_a_long_chain() {
    assert_runs_with_output(
        r#"
use system.io

class Tally
    public var seen int
    public var ordered bool

class Node
    public var next Node?
    public var v int
    public var tally Tally

    fn init(v int, tally Tally)
        self.v = v
        self.tally = tally

    public fn drop(self)
        if self.v != self.tally.seen
            self.tally.ordered = false
        self.tally.seen = self.tally.seen - 1

fn build(t Tally)
    var head = Node(0, t)
    for i in 1..1000000
        var n = Node(i, t)
        n.next = head
        head = n

fn main()
    let t = Tally(seen: 999999, ordered: true)
    build(t)
    println(f"{t.seen} {t.ordered}")
"#,
        "-1 true",
    );
}

#[test]
fn test_a_chain_through_list_fields_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var kids List<Node>
    var v int
    fn init(v int)
        self.v = v
        self.kids = List<Node>()

fn main()
    var head = Node(0)
    for i in 1..1000000
        var n = Node(i)
        n.kids.push(head)
        head = n
    println(f"{head.v}")
"#,
        "999999",
    );
}

#[test]
fn test_a_chain_through_map_values_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

class Node
    var kids Map<int, Node>
    var v int
    fn init(v int)
        self.v = v
        self.kids = Map<int, Node>()

fn main()
    var head = Node(0)
    for i in 1..500000
        var n = Node(i)
        n.kids.set(0, head)
        head = n
    println(f"{head.v}")
"#,
        "499999",
    );
}

#[test]
fn test_a_chain_through_an_enum_payload_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

enum Chain
    End
    Link(int, Chain)

fn main()
    var head = Chain.End
    for i in 0..2000000
        head = Chain.Link(i, head)
    match head
        Chain.Link(v, _): println(f"{v}")
        Chain.End: println("empty")
"#,
        "1999999",
    );
}

#[test]
fn test_a_chain_past_the_stack_of_closures_captured_by_closures_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

fn wrap(f fn() int) fn() int
    return fn() int: f()

fn main()
    var head = fn() int: 7
    for i in 0..2000000
        head = wrap(head)
    println("built")
"#,
        "built",
    );
}

/// A chain short enough to fit, whose objects also hold shared members: each
/// member survives the release of the objects that held it.
#[test]
fn test_a_release_leaves_a_member_something_else_still_holds() {
    assert_heap_guard_output(
        r#"
use system.io

class Payload
    public var v int

class Node
    public var next Node?
    public var p Payload

    fn init(p Payload)
        self.p = p

fn main()
    let held = Payload(v: 41)
    var head = Node(held)
    for i in 1..1000
        var n = Node(held)
        n.next = head
        head = n
    head = Node(held)
    println(f"{held.v + 1}")
"#,
        "42",
    );
}

#[test]
fn test_a_chain_of_two_classes_holding_each_other_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class A
    var b B?
    var v int
    fn init(v int)
        self.v = v

class B
    var a A?
    var v int
    fn init(v int)
        self.v = v

fn main()
    var head = A(0)
    for i in 1..1000000
        var b = B(i)
        b.a = head
        var a = A(i)
        a.b = b
        head = a
    println(f"{head.v}")
"#,
        "999999",
    );
}

#[test]
fn test_a_chain_through_a_base_class_field_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class Base
    var next Base?
    var v int
    fn init(v int)
        self.v = v

class Derived extends Base
    var w int
    fn init(v int)
        super.init(v)
        self.w = v

fn main()
    var head Base = Derived(0)
    for i in 1..1000000
        var n = Derived(i)
        n.next = head
        head = n
    println("built")
"#,
        "built",
    );
}

#[test]
fn test_a_chain_through_a_trait_member_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

trait Link
    fn id() int

class End implements Link
    fn id() int
        0

class Node implements Link
    var next Link
    var v int
    fn init(v int, next Link)
        self.v = v
        self.next = next

    fn id() int
        self.v

fn main()
    var head Link = End()
    for i in 1..1000000
        head = Node(i, head)
    println(f"{head.id()}")
"#,
        "999999",
    );
}

#[test]
fn test_a_chain_through_a_generic_box_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    var value T?

class Node
    var next Box<Node>
    var v int
    fn init(v int, next Box<Node>)
        self.v = v
        self.next = next

fn main()
    var head = Node(0, Box<Node>())
    for i in 1..1000000
        var b = Box<Node>()
        b.value = head
        head = Node(i, b)
    println(f"{head.v}")
"#,
        "999999",
    );
}

/// A class that holds a chain without being part of one still releases the
/// chain safely: the guard is on the chain's own class.
#[test]
fn test_a_flat_class_holding_a_long_chain_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var next Node?
    var v int
    fn init(v int)
        self.v = v

class Holder
    var head Node
    fn init(head Node)
        self.head = head

fn main()
    var head = Node(0)
    for i in 1..2000000
        var n = Node(i)
        n.next = head
        head = n
    let holder = Holder(head)
    println(f"{holder.head.v}")
"#,
        "1999999",
    );
}

/// The instance a scalar argument makes flat releases as flat, and prints the
/// value it held.
#[test]
fn test_a_generic_box_at_a_scalar_type_argument_releases_cleanly() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    public var value T

fn main()
    let b = Box<int>(value: 7)
    println(f"{b.value}")
"#,
        "7",
    );
}
