// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_super_method_calls_self_method() {
    // Parent method calls another method on self — verifies self pointer is valid (not null)
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn label() String
        "animal"
    fn speak()
        println(self.label())

class Dog extends Animal
    var breed String
    fn speak()
        super.speak()
        println("woof")

fn main()
    let d = Dog()
    d.speak()
    "#,
        "animal\nwoof",
    );
}

// ── Super calls that read/write self fields ───────────────────────────────────

#[test]
fn test_super_method_reads_own_field_via_self() {
    // The parent method reads `self.count`; child's super.show() must pass the
    // real self pointer so the value set in child's init is visible.
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int
    fn init(c int)
        self.count = c
    fn show()
        println(f"{self.count}")

class DoubleCounter extends Counter
    fn showViaSuper()
        super.show()

fn main()
    let dc = DoubleCounter(c: 7)
    dc.showViaSuper()
    "#,
        "7",
    );
}

#[test]
fn test_super_method_modifies_field_visible_in_child() {
    // Parent method mutates `self.value`; after super.reset() the child sees 0.
    assert_runs_with_output(
        r#"
use system.io

class Base
    var value int
    fn init(v int)
        self.value = v
    fn reset()
        self.value = 0

class Child extends Base
    fn resetAndPrint()
        super.reset()
        println(f"{self.value}")

fn main()
    let c = Child(v: 42)
    c.resetAndPrint()
    "#,
        "0",
    );
}

#[test]
fn test_super_method_call_basic() {
    // Dog.speak() calls super.speak() then prints its own message
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
        super.speak()
        println("woof")

fn main()
    let d = Dog()
    d.speak()
    "#,
        "animal\nwoof",
    );
}

#[test]
fn test_super_method_call_only() {
    // Dog.speak() only calls super.speak() — output should be just the parent's
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
        super.speak()

fn main()
    let d = Dog()
    d.speak()
    "#,
        "animal",
    );
}

#[test]
fn test_super_method_with_return_value() {
    // Child adds 1 to parent's returned value via super call
    assert_runs_with_output(
        r#"
use system.io

class Shape
    var id int
    fn area() int
        10

class Circle extends Shape
    var radius int
    fn area() int
        let base = super.area()
        base + 5

fn main()
    let c = Circle()
    println(f"{c.area()}")
    "#,
        "15",
    );
}

#[test]
fn test_super_method_multi_level() {
    // Poodle.speak() → super.speak() resolves to Dog.speak() (not Animal.speak())
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
    fn speak()
        super.speak()
        println("poodle")

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "dog\npoodle",
    );
}

#[test]
fn test_super_method_skips_one_level() {
    // Poodle.speak() → super.speak() → Dog.speak() → super.speak() → Animal.speak()
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
        super.speak()
        println("dog")

class Poodle extends Dog
    var size String
    fn speak()
        super.speak()
        println("poodle")

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "animal\ndog\npoodle",
    );
}
