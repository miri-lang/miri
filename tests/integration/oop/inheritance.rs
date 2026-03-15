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
