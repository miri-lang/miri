// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A constructor argument read straight off another instance's field is
//! passed at that field's own type, so an optional field handed to an optional
//! field is stored as it is, never wrapped in a second `Some`.

use crate::integration::utils::*;

const NODE_AND_PAIR: &str = r#"
use system.io

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

class Pair<A, B>
    var a A?
    var b B?
    fn drop()
        println("drop pair")

    fn swap() Pair<B, A>
        Pair<B, A>(a: self.b, b: self.a)
"#;

#[test]
fn a_generic_instance_built_from_another_ones_optional_fields_releases_everything() {
    assert_heap_guard_output(
        &format!(
            "{NODE_AND_PAIR}
fn main()
    let p = Pair<Node, int>(a: Some(Node(name: \"c\" + \"1\")), b: Some(3))
    let q = Pair<int, Node>(a: p.b, b: p.a)
    match q.b
        Some(n): println(f\"got {{n.name}}\")
        None: println(\"none\")
    println(f\"{{q.a ?? 0}}\")
"
        ),
        "got c1\n3\ndrop pair\ndrop pair\ndrop c1",
    );
}

#[test]
fn a_generic_method_building_its_swapped_instance_releases_everything() {
    assert_heap_guard_output(
        &format!(
            "{NODE_AND_PAIR}
fn main()
    let p = Pair<Node, int>(a: Some(Node(name: \"s\" + \"1\")), b: None)
    let q = p.swap()
    match q.b
        Some(n): println(f\"got {{n.name}}\")
        None: println(\"none\")
    println(f\"{{q.a ?? -1}}\")
"
        ),
        "got s1\n-1\ndrop pair\ndrop pair\ndrop s1",
    );
}

#[test]
fn a_class_built_from_another_instances_optional_field_stores_it_unwrapped() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String

class Slot
    var held Node?
    var count int?

fn main()
    let first = Slot(held: Some(Node(name: "n" + "1")), count: Some(5))
    let second = Slot(held: first.held, count: first.count)
    match second.held
        Some(n): println(n.name)
        None: println("none")
    println(f"{second.count ?? 0}")
"#,
        "n1\n5",
    );
}

#[test]
fn a_struct_built_from_another_structs_optional_fields_stores_them_unwrapped() {
    assert_heap_guard_output(
        r#"
use system.io

struct Entry
    label String?
    weight int?

fn main()
    let first = Entry(label: Some("e" + "1"), weight: None)
    let second = Entry(label: first.label, weight: first.weight)
    println(second.label ?? "none")
    println(f"{second.weight ?? -1}")
"#,
        "e1\n-1",
    );
}

#[test]
fn a_class_built_from_optional_list_elements_stores_them_unwrapped() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String

class Slot
    var held Node?

fn main()
    var xs = List<Node?>()
    xs.push(Some(Node(name: "l" + "1")))
    xs.push(None)
    let a = Slot(held: xs[0])
    let b = Slot(held: xs[1])
    match a.held
        Some(n): println(n.name)
        None: println("none")
    match b.held
        Some(n): println(n.name)
        None: println("none")
"#,
        "l1\nnone",
    );
}
