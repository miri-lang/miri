// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A trait default inherited by a generic class that builds a generic class
//! instance at the trait's parameter.
//!
//! The class's copy of the default is compiled at each instantiation that
//! reaches it. A program that declares the generic class but never reaches
//! the default, while calling the built class's method through a trait value
//! elsewhere, compiles and runs.

use super::utils::*;

const MAKER: &str = r#"
use system.io

trait Next<T>
    fn next() T?

class Box<T> implements Next<T>
    var v T
    fn init(v T)
        self.v = v
    public fn next() T?
        return Some(self.v)

trait Maker<T>
    fn get() T
    fn make() Next<T>
        return Box<T>(v: self.get())

class Wrap<T> implements Maker<T>
    var v T
    fn init(v T)
        self.v = v
    public fn get() T: self.v
"#;

/// `MAKER` followed by `main`.
fn program(main: &str) -> String {
    format!("{MAKER}\n{main}")
}

#[test]
fn unreached_default_on_a_generic_class_leaves_a_trait_call_compiling() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let n Next<int> = Box<int>(v: 7)
    println(f"{n.next() ?? -1}")
"#,
        ),
        "7",
    );
}

#[test]
fn default_called_at_an_int_instance_builds_its_value() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let n = Wrap<int>(v: 5).make()
    println(f"{n.next() ?? -1}")
"#,
        ),
        "5",
    );
}

#[test]
fn default_called_at_a_string_instance_builds_its_value() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let n = Wrap<String>(v: "fi" + "ve").make()
    println(n.next() ?? "none")
"#,
        ),
        "five",
    );
}

#[test]
fn default_reached_through_the_generic_classes_own_method() {
    assert_heap_guard_output(
        r#"
use system.io

trait Next<T>
    fn next() T?

class Box<T> implements Next<T>
    var v T
    fn init(v T)
        self.v = v
    public fn next() T?
        return Some(self.v)

trait Maker<T>
    fn get() T
    fn make() Next<T>
        return Box<T>(v: self.get())

class Wrap<T> implements Maker<T>
    var v T
    fn init(v T)
        self.v = v
    public fn get() T: self.v
    public fn first() T?
        return self.make().next()

fn main()
    let w = Wrap<int>(v: 9)
    println(f"{w.first() ?? -1}")
"#,
        "9",
    );
}

#[test]
fn default_reached_in_a_shared_generic_body() {
    assert_heap_guard_output(
        &program(
            r#"
fn first<M implements Maker<int>>(m M) int
    return m.make().next() ?? -1

fn main()
    println(f"{first(Wrap<int>(v: 4))}")
"#,
        ),
        "4",
    );
}
