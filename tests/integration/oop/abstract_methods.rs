// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Happy-path: abstract class with single abstract method ──────────────────

#[test]
fn test_abstract_method_override_executes() {
    // Concrete subclass overrides abstract fn speak() — must call Dog_speak.
    assert_runs_with_output(
        r#"
use system.io

abstract class Animal
    abstract fn speak()

class Dog extends Animal
    fn speak()
        println("woof")

fn main()
    let d = Dog()
    d.speak()
    "#,
        "woof",
    );
}

#[test]
fn test_two_subclasses_each_override_abstract() {
    // Two concrete subclasses — each must call their own override.
    assert_runs_with_output(
        r#"
use system.io

abstract class Shape
    abstract fn area() int

class Square extends Shape
    var side int
    fn init(s int)
        self.side = s
    fn area() int
        self.side * self.side

class Triangle extends Shape
    fn area() int
        3

fn main()
    let sq = Square(s: 4)
    let tr = Triangle()
    println(f"{sq.area()}")
    println(f"{tr.area()}")
    "#,
        "16\n3",
    );
}

#[test]
fn test_abstract_class_concrete_method_inherited() {
    // Abstract class has a concrete method alongside an abstract one.
    // The concrete method is inherited by the subclass and works independently.
    assert_runs_with_output(
        r#"
use system.io

abstract class Vehicle
    abstract fn fuel_type() String
    fn category()
        println("vehicle")

class Car extends Vehicle
    fn fuel_type() String
        "petrol"

fn main()
    let c = Car()
    c.category()
    println(c.fuel_type())
    "#,
        "vehicle\npetrol",
    );
}

#[test]
fn test_abstract_chain_intermediate_abstract() {
    // Abstract -> Abstract -> Concrete.
    // Only the leaf class is concrete and provides the override.
    assert_runs_with_output(
        r#"
use system.io

abstract class Base
    abstract fn greet()

abstract class Middle extends Base

class Leaf extends Middle
    fn greet()
        println("hello from leaf")

fn main()
    let l = Leaf()
    l.greet()
    "#,
        "hello from leaf",
    );
}

#[test]
fn test_multiple_abstract_methods_all_overridden() {
    // Concrete class must override all abstract methods.
    assert_runs_with_output(
        r#"
use system.io

abstract class Printer
    abstract fn header()
    abstract fn body()
    abstract fn footer()

class Report extends Printer
    fn header()
        println("=== start ===")
    fn body()
        println("content")
    fn footer()
        println("=== end ===")

fn main()
    let r = Report()
    r.header()
    r.body()
    r.footer()
    "#,
        "=== start ===\ncontent\n=== end ===",
    );
}

#[test]
fn test_abstract_method_with_parameters() {
    // Abstract method with parameters and return value.
    assert_runs_with_output(
        r#"
use system.io

abstract class Adder
    abstract fn add(a int, b int) int

class SimpleAdder extends Adder
    fn add(a int, b int) int
        a + b

fn main()
    let adder = SimpleAdder()
    println(f"{adder.add(3, 4)}")
    "#,
        "7",
    );
}

// ── Virtual dispatch: polymorphic variable ───────────────────────────────────

#[test]
fn test_virtual_dispatch_via_abstract_variable() {
    // `let a Animal = Dog()` — method call on abstract-typed variable must
    // dispatch to the concrete override at runtime.
    assert_runs_with_output(
        r#"
use system.io

abstract class Animal
    abstract fn speak()

class Dog extends Animal
    fn speak()
        println("woof")

fn main()
    let a Animal = Dog()
    a.speak()
    "#,
        "woof",
    );
}

#[test]
fn test_virtual_dispatch_multiple_concrete_types() {
    // Multiple Animal-typed variables of different concrete types — each call
    // must go to the right override at runtime.
    assert_runs_with_output(
        r#"
use system.io

abstract class Animal
    abstract fn speak()

class Dog extends Animal
    fn speak()
        println("woof")

class Cat extends Animal
    fn speak()
        println("meow")

fn main()
    let a1 Animal = Dog()
    let a2 Animal = Cat()
    let a3 Animal = Dog()
    a1.speak()
    a2.speak()
    a3.speak()
    "#,
        "woof\nmeow\nwoof",
    );
}

#[test]
fn test_concrete_method_in_abstract_calls_abstract_method() {
    // A concrete method in the abstract class calls `self.abstractMethod()`.
    // It must resolve to the subclass override without a linker error.
    assert_runs_with_output(
        r#"
use system.io

abstract class Greeter
    abstract fn name() String
    fn greet()
        println(f"Hello, {self.name()}!")

class Greeter1 extends Greeter
    fn name() String
        "World"

class Greeter2 extends Greeter
    fn name() String
        "Miri"

fn main()
    let g1 Greeter = Greeter1()
    let g2 Greeter = Greeter2()
    g1.greet()
    g2.greet()
    "#,
        "Hello, World!\nHello, Miri!",
    );
}

// ── Error cases: type checker must reject these ──────────────────────────────

#[test]
fn test_abstract_class_cannot_be_instantiated() {
    assert_compiler_error(
        r#"
abstract class Animal
    abstract fn speak()

fn main()
    let a = Animal()
    "#,
        "Cannot instantiate abstract class",
    );
}

#[test]
fn test_non_abstract_class_missing_override_is_error() {
    assert_compiler_error(
        r#"
abstract class Animal
    abstract fn speak()

class Dog extends Animal

fn main()
    let d = Dog()
    "#,
        "Class 'Dog' must implement abstract method 'speak' from class 'Animal'",
    );
}

#[test]
fn test_non_abstract_class_cannot_declare_abstract_method() {
    assert_compiler_error(
        r#"
class Animal
    abstract fn speak()

fn main()
    let a = Animal()
    "#,
        "abstract method",
    );
}
