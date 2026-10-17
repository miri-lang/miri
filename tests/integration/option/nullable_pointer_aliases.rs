// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An alias is laid out as the type it names, at the arguments it is written
//! with: an optional of `Id<Node>` (`type Id<T> is T`) is the same nullable
//! `Node` pointer as `Node?`, `Opt<Node>` (`type Opt<T> is T?`) is `Node?`,
//! and an optional of an alias naming `Node?` is `Node??`. Values pass
//! between the spellings, and each is released once, by its own drop hook.

use super::utils::*;

#[test]
fn test_an_alias_naming_an_optional_class_is_that_optional() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println("drop node")

type MaybeNode is Node?

fn show(m MaybeNode) String
    match m
        Some(n): n.name
        None: "none"

fn main()
    let x MaybeNode = Some(Node(name: "a" + "b"))
    println(show(x))
    match x
        Some(n): println(n.name)
        None: println("none")
    let y Node? = x
    println(show(y))
    let z MaybeNode? = Some(x)
    match z
        Some(inner): println(show(inner))
        None: println("outer none")
"#,
        "ab\nab\nab\nab\ndrop node",
    );
}

#[test]
fn test_an_alias_naming_an_optional_class_holds_it_in_fields_lists_and_tuples() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

type MaybeNode is Node?

class H
    var m MaybeNode

fn main()
    let h = H(m: Some(Node(name: "a" + "b")))
    match h.m
        Some(n): println(n.name)
        None: println("none")
    var xs = List<MaybeNode>()
    xs.push(Some(Node(name: "c" + "d")))
    match xs[0]
        Some(n): println(n.name)
        None: println("none")
    let t (MaybeNode, int) = (Some(Node(name: "e" + "f")), 1)
    match t.0
        Some(n): println(n.name)
        None: println("none")
"#,
        "ab\ncd\nef\ndrop ef\ndrop cd\ndrop ab",
    );
}

#[test]
fn test_an_optional_of_a_generic_alias_of_a_class_runs_the_drop_hook() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println("drop node")

type Id<T> is T

fn main()
    let a Id<Node>? = Some(Node(name: "a" + "b"))
    match a
        Some(n): println(n.name)
        None: println("none")
"#,
        "ab\ndrop node",
    );
}

#[test]
fn test_an_optional_of_a_generic_alias_of_a_class_is_released_as_that_class() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn label() String

class Node implements Named
    var name String
    fn label() String
        self.name

type Id<T> is T

fn main()
    let a Id<Node>? = Some(Node(name: "a" + "b"))
    match a
        Some(n): println(n.label())
        None: println("none")
"#,
        "ab",
    );
}

#[test]
fn test_an_optional_of_a_generic_alias_flows_through_fields_lists_and_parameters() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

type Id<T> is T

class H
    var m Id<Node>?

fn show(o Id<Node>?) String
    match o
        Some(n): n.name
        None: "none"

fn main()
    let h = H(m: Some(Node(name: "a" + "b")))
    println(show(h.m))
    var xs = List<Id<Node>?>()
    xs.push(Some(Node(name: "c" + "d")))
    xs.push(None)
    println(show(xs[0]))
    println(show(xs[1]))
    let plain Node? = Some(Node(name: "e" + "f"))
    println(show(plain))
"#,
        "ab\ncd\nnone\nef\ndrop ef\ndrop cd\ndrop ab",
    );
}

#[test]
fn test_a_generic_alias_naming_an_optional_is_that_optional_at_its_argument() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var name String
    fn drop()
        println(f"drop {self.name}")

type Opt<T> is T?

class H
    var m Opt<Node>

fn show(o Opt<Node>) String
    match o
        Some(n): n.name
        None: "none"

fn main()
    let a Opt<Node> = Some(Node(name: "a" + "b"))
    println(show(a))
    let h = H(m: Some(Node(name: "c" + "d")))
    match h.m
        Some(n): println(n.name)
        None: println("none")
    let nn Node? = Some(Node(name: "e" + "f"))
    println(show(nn))
    let empty Opt<Node> = None
    println(show(empty))
"#,
        "ab\ncd\nef\nnone\ndrop ef\ndrop cd\ndrop ab",
    );
}
