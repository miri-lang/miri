// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A class instance held at a base class or a trait is released as the class
//! it was built as: its own drop hook runs and its own managed fields are
//! released, not the ones the static type knows about.

use super::super::utils::*;

const ANIMALS: &str = r#"
use system.collections.list
use system.collections.array

class Animal
    n int
    fn init(n int)
        self.n = n

class Dog extends Animal
    tag String
    fn init(t String)
        super.init(1)
        self.tag = t
"#;

fn program(main: &str) -> String {
    format!("{ANIMALS}\n{main}")
}

#[test]
fn test_a_subclass_in_an_array_of_its_base_releases_its_own_fields() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let xs Array<Animal, 2> = [Dog("a" + "b"), Dog("c" + "d")]
    println(f"{xs[0].n}")
"#,
        ),
        "1",
    );
}

#[test]
fn test_a_subclass_bound_at_its_base_type_releases_its_own_fields() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    let a Animal = Dog("a" + "b")
    let plain = Animal(7)
    println(f"{a.n} {plain.n}")
"#,
        ),
        "1 7",
    );
}

#[test]
fn test_a_subclass_in_a_list_of_its_base_releases_its_own_fields() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    var xs = List<Animal>()
    xs.push(Dog("a" + "b"))
    xs.push(Animal(2))
    println(f"{xs[0].n} {xs[1].n}")
"#,
        ),
        "1 2",
    );
}

#[test]
fn test_a_generic_class_in_a_list_of_a_trait_links_and_releases() {
    assert_heap_guard_output(
        r#"
use system.collections.list

trait Named
    fn name() String
    fn shout() String
        return self.name() + "!"

class N<T> implements Named
    v T
    fn name() String
        return "n"

class Tagged implements Named
    tag String
    fn name() String
        return self.tag

fn main()
    var ns = List<Named>()
    ns.push(N<int>(v: 1))
    ns.push(Tagged(tag: "t" + "g"))
    println(f"{ns[0].shout()} {ns[1].shout()}")
"#,
        "n! tg!",
    );
}

#[test]
fn test_a_plain_class_bound_at_a_trait_releases_its_fields() {
    assert_heap_guard_output(
        r#"
trait Get<T>
    fn get() T

class Named implements Get<String>
    v String
    fn get() String
        return self.v

fn main()
    let s Get<String> = Named(v: "a" + "b")
    println(s.get())
"#,
        "ab",
    );
}
