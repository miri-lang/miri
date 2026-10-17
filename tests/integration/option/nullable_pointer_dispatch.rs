// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional handed back through a virtual call is laid out as its caller
//! reads it: a trait default returning `Self?` or an optional of the trait's
//! parameter answers a call through a trait-typed value with the body compiled
//! for the implementor's own arguments.

use crate::integration::utils::*;

#[test]
fn a_trait_default_returning_self_optional_answers_a_dynamic_call() {
    assert_heap_guard_output(
        r#"
use system.io

trait Shape
    fn area() int
    fn me() Self?
        if self.area() > 0
            return Some(self)
        None

class Sq implements Shape
    var s int
    fn area() int
        self.s * self.s

fn report(sh Shape)
    match sh.me()
        Some(v): println(f"some {v.area()}")
        None: println("none")

fn main()
    report(Sq(s: 3))
    report(Sq(s: 0))
    match Sq(s: 2).me()
        Some(q): println(f"direct {q.area()}")
        None: println("none")
"#,
        "some 9\nnone\ndirect 4",
    );
}

#[test]
fn a_trait_default_returning_an_optional_of_its_parameter_answers_generic_and_concrete_implementors(
) {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Node
    var name String

trait Source<T>
    fn items() List<T>
    fn first_or_none() T?
        let xs = self.items()
        if xs.length() == 0
            return None
        Some(xs[0])

class Holder<T> implements Source<T>
    var xs List<T>
    fn items() List<T>
        self.xs

class Plain implements Source<Node>
    var xs List<Node>
    fn items() List<Node>
        self.xs

fn show(s Source<Node>)
    match s.first_or_none()
        Some(n): println(f"got {n.name}")
        None: println("none")

fn main()
    var xs = List<Node>()
    xs.push(Node(name: "a" + "1"))
    show(Holder<Node>(xs: xs))
    show(Plain(xs: xs))
    show(Plain(xs: List<Node>()))
    let ints = Holder<int>(xs: List<int>())
    println(f"{ints.first_or_none() ?? -1}")
"#,
        "got a1\ngot a1\nnone\n-1",
    );
}
