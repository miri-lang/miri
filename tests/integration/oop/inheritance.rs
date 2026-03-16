// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_inherited_method_basic() {
    // Dog extends Animal but does not override speak — must resolve to Animal_speak
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String

fn main()
    let d = Dog()
    d.speak()
    "#,
        "animal",
    );
}

#[test]
fn test_overridden_method_uses_child_impl() {
    // Dog overrides speak — must call Dog_speak, not Animal_speak
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn speak()
        println("dog")

fn main()
    let d = Dog()
    d.speak()
    "#,
        "dog",
    );
}

#[test]
fn test_multi_level_inheritance_calls_grandparent() {
    // Poodle extends Dog extends Animal — speak only on Animal
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String

class Poodle extends Dog
    var size String

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "animal",
    );
}

#[test]
fn test_override_at_middle_level_of_chain() {
    // Poodle extends Dog extends Animal — Dog overrides speak, Poodle does not
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn speak()
        println("dog")

class Poodle extends Dog
    var size String

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "dog",
    );
}

#[test]
fn test_inherited_method_with_return_value() {
    // Inherited method that returns int
    assert_runs_with_output(
        r#"
use system.io

class Shape
    var id int
    fn area() int
        42

class Circle extends Shape
    var radius int

fn main()
    let c = Circle()
    println(f"{c.area()}")
    "#,
        "42",
    );
}

#[test]
fn test_both_own_and_inherited_methods() {
    // Dog has its own method AND inherits speak from Animal
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn fetch()
        println("fetching")

fn main()
    let d = Dog()
    d.speak()
    d.fetch()
    "#,
        "animal\nfetching",
    );
}

#[test]
fn test_inherited_init_no_own_fields() {
    // Dog has no own fields; constructor must detect and call Animal_init
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn init(n String)
        self.name = n

class Dog extends Animal

fn main()
    let d = Dog(n: "Rex")
    println(d.name)
    "#,
        "Rex",
    );
}

#[test]
fn test_parent_method_reads_inherited_field() {
    // Animal.speak reads self.name; must use correct offset when called on Dog instance
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn init(n String)
        self.name = n
    fn speak()
        println(self.name)

class Dog extends Animal

fn main()
    let d = Dog(n: "Rex")
    d.speak()
    "#,
        "Rex",
    );
}

#[test]
fn test_subclass_method_reads_inherited_field() {
    // DoubleCounter.doubled() reads self.count which is an inherited field
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int
    fn init(c int)
        self.count = c

class DoubleCounter extends Counter
    fn doubled() int
        self.count * 2

fn main()
    let dc = DoubleCounter(c: 5)
    println(f"{dc.doubled()}")
    "#,
        "10",
    );
}

#[test]
fn test_field_layout_base_fields_before_derived() {
    // Base class fields must come before derived class fields in memory layout.
    // Dog has own field `breed`; Animal has `name`. Full layout: [name, breed].
    // Animal.init sets self.name; Dog.init calls super.init then sets self.breed.
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn init(n String)
        self.name = n

class Dog extends Animal
    var breed String
    fn init(n String, b String)
        super.init(n)
        self.breed = b
    fn describe()
        println(self.name)
        println(self.breed)

fn main()
    let d = Dog(n: "Rex", b: "Lab")
    d.describe()
    "#,
        "Rex\nLab",
    );
}
