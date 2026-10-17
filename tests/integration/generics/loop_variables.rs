// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `for` loop inside a generic body binds its element at the type the body
//! was instantiated at, so a managed element is released once per pass.

use crate::integration::utils::*;

const PICK: &str = r#"
use system.io
use system.collections.list

fn pick<T>(xs List<T>) T?
    var best T? = None
    for x in xs
        best = Some(x)
    best
"#;

#[test]
fn a_generic_loop_releases_each_string_element_it_wraps() {
    assert_heap_guard_output(
        &format!(
            "{PICK}
fn main()
    var xs = List<String>()
    xs.push(\"a\" + \"1\")
    xs.push(\"b\" + \"2\")
    xs.push(\"c\" + \"3\")
    match pick(xs)
        Some(s): println(s)
        None: println(\"none\")
"
        ),
        "c3",
    );
}

#[test]
fn a_generic_loop_releases_each_class_element_it_wraps() {
    assert_heap_guard_output(
        &format!(
            "{PICK}
class Node
    var name String
    fn drop()
        println(f\"drop {{self.name}}\")

fn main()
    var xs = List<Node>()
    xs.push(Node(name: \"a\" + \"1\"))
    xs.push(Node(name: \"b\" + \"2\"))
    match pick(xs)
        Some(n): println(f\"got {{n.name}}\")
        None: println(\"none\")
    println(\"end\")
"
        ),
        "got b2\nend\ndrop a1\ndrop b2",
    );
}

#[test]
fn a_generic_loop_over_integers_picks_the_last() {
    assert_heap_guard_output(
        &format!(
            "{PICK}
fn main()
    var xs = List<int>()
    xs.push(4)
    xs.push(7)
    println(f\"{{pick(xs) ?? 0}}\")
"
        ),
        "7",
    );
}

#[test]
fn a_generic_loop_over_a_map_releases_each_key_and_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn longest<K, V>(m Map<K, V>) int
    var n = 0
    for k, v in m
        let held = Some(k)
        let also = Some(v)
        n = n + 1
    n

fn main()
    var m = Map<String, String>()
    m["a" + "1"] = "x" + "1"
    m["b" + "2"] = "y" + "2"
    println(f"{longest(m)}")
"#,
        "2",
    );
}

#[test]
fn a_generic_method_loop_releases_its_elements_on_an_early_return() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Bag<T>
    var items List<T>

    fn first_after(skip int) T?
        var seen = 0
        for x in self.items
            if seen == skip
                return Some(x)
            seen = seen + 1
        None

    fn last_before_break(stop int) T?
        var best T? = None
        var seen = 0
        for x in self.items
            if seen == stop
                break
            best = Some(x)
            seen = seen + 1
        best

fn main()
    var xs = List<String>()
    xs.push("a" + "1")
    xs.push("b" + "2")
    xs.push("c" + "3")
    let bag = Bag<String>(items: xs)
    println(bag.first_after(1) ?? "none")
    println(bag.last_before_break(2) ?? "none")
    println(bag.first_after(5) ?? "none")
"#,
        "b2\nb2\nnone",
    );
}
