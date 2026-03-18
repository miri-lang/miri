// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Basic trait definition & implementation ───────────────────────────────────

#[test]
fn test_trait_basic_definition_and_implementation() {
    // A class implements a trait and the method is callable on a concrete instance.
    assert_runs_with_output(
        r#"
use system.io

trait Greetable
    fn greet()

class Person implements Greetable
    fn greet()
        println("hello")

fn main()
    let p = Person()
    p.greet()
    "#,
        "hello",
    );
}

#[test]
fn test_trait_method_with_return_value() {
    // Trait method returns a value; concrete class provides the implementation.
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name() String

class Cat implements Named
    fn name() String
        "Whiskers"

fn main()
    let c = Cat()
    println(c.name())
    "#,
        "Whiskers",
    );
}

#[test]
fn test_trait_method_with_parameters() {
    // Trait method takes parameters; the implementing class must match the signature.
    assert_runs_with_output(
        r#"
use system.io

trait Adder
    fn add(a int, b int) int

class Calc implements Adder
    fn add(a int, b int) int
        a + b

fn main()
    let c = Calc()
    println(f"{c.add(3, 4)}")
    "#,
        "7",
    );
}

// ── Default (concrete) methods in traits ─────────────────────────────────────

#[test]
fn test_trait_default_method_used_when_not_overridden() {
    // A trait method that has a body provides a default implementation.
    // A class that does not override it must inherit the default.
    // Fix: when resolving a method call on a class, also walk its `implements`
    // list and check for a concrete (non-abstract) method in the trait body.
    assert_runs_with_output(
        r#"
use system.io

trait Printable
    fn print()
        println("default print")

class Widget implements Printable

fn main()
    let w = Widget()
    w.print()
    "#,
        "default print",
    );
}

#[test]
fn test_trait_default_method_overridden_by_class() {
    // When the class provides its own implementation it must take precedence.
    assert_runs_with_output(
        r#"
use system.io

trait Printable
    fn print()
        println("default")

class Fancy implements Printable
    fn print()
        println("fancy")

fn main()
    let f = Fancy()
    f.print()
    "#,
        "fancy",
    );
}

// ── Multiple trait implementation ─────────────────────────────────────────────

#[test]
fn test_class_implements_two_traits() {
    // A class may implement multiple traits; all methods must be reachable.
    assert_runs_with_output(
        r#"
use system.io

trait Runnable
    fn run()

trait Flyable
    fn fly()

class SuperHero implements Runnable, Flyable
    fn run()
        println("running")
    fn fly()
        println("flying")

fn main()
    let s = SuperHero()
    s.run()
    s.fly()
    "#,
        "running\nflying",
    );
}

#[test]
fn test_class_extends_and_implements_trait() {
    // A class can both extend a base class AND implement a trait.
    assert_runs_with_output(
        r#"
use system.io

class Animal
    fn breathe()
        println("breathing")

trait Swimmer
    fn swim()

class Fish extends Animal implements Swimmer
    fn swim()
        println("swimming")

fn main()
    let f = Fish()
    f.breathe()
    f.swim()
    "#,
        "breathing\nswimming",
    );
}

// ── Trait inheritance ─────────────────────────────────────────────────────────

#[test]
fn test_child_trait_inherits_parent_trait_methods() {
    // `trait B extends A` — a class implementing B must also provide A's methods.
    assert_runs_with_output(
        r#"
use system.io

trait Shape
    fn area() int

trait ColoredShape extends Shape
    fn color() String

class RedSquare implements ColoredShape
    fn area() int
        9
    fn color() String
        "red"

fn main()
    let s = RedSquare()
    println(f"{s.area()}")
    println(s.color())
    "#,
        "9\nred",
    );
}

#[test]
fn test_trait_chain_three_levels() {
    // Three-level trait chain: C extends B extends A.
    // Class implementing C must provide all methods from the entire chain.
    assert_runs_with_output(
        r#"
use system.io

trait A
    fn a()

trait B extends A
    fn b()

trait C extends B
    fn c()

class Impl implements C
    fn a()
        println("a")
    fn b()
        println("b")
    fn c()
        println("c")

fn main()
    let x = Impl()
    x.a()
    x.b()
    x.c()
    "#,
        "a\nb\nc",
    );
}

// ── Trait as variable / parameter type (polymorphic dispatch) ─────────────────
//
// BUG: Virtual dispatch through a trait-typed variable is NOT yet implemented.
// `class_needs_vtable` in context.rs only walks the `extends` chain for
// abstract ancestors — it does NOT check the `implements` list.
// The type checker also rejects member access on a trait-typed variable with
// "Type 'X' does not have members".
//
// Fix required: extend `class_needs_vtable` to return true when a class
// implements at least one trait, generate a vtable for trait methods, and
// allow the type checker to resolve method calls on trait-typed variables.

#[test]
fn test_trait_typed_variable_dispatches_correctly() {
    // `let x Greetable = Greeter()` — method call on a trait-typed variable must
    // dispatch to the concrete implementation at runtime (needs trait vtable).
    assert_runs_with_output(
        r#"
use system.io

trait Greetable
    fn greet()

class Greeter implements Greetable
    fn greet()
        println("hi")

fn main()
    let x Greetable = Greeter()
    x.greet()
    "#,
        "hi",
    );
}

#[test]
fn test_trait_typed_variable_two_implementations() {
    // Two different concrete types behind a shared trait-typed variable.
    assert_runs_with_output(
        r#"
use system.io

trait Speaker
    fn speak()

class Dog implements Speaker
    fn speak()
        println("woof")

class Cat implements Speaker
    fn speak()
        println("meow")

fn main()
    let a Speaker = Dog()
    let b Speaker = Cat()
    a.speak()
    b.speak()
    "#,
        "woof\nmeow",
    );
}

#[test]
fn test_function_accepting_trait_parameter() {
    // A function whose parameter type is a trait — concrete object passed in.
    assert_runs_with_output(
        r#"
use system.io

trait Describable
    fn describe() String

class Tree implements Describable
    fn describe() String
        "oak"

fn print_description(d Describable)
    println(d.describe())

fn main()
    let t = Tree()
    print_description(t)
    "#,
        "oak",
    );
}

// ── Error cases ───────────────────────────────────────────────────────────────

#[test]
fn test_missing_required_trait_method_is_error() {
    // Class declares `implements` but does not provide the required method.
    assert_compiler_error(
        r#"
trait Runnable
    fn run()

class Lazy implements Runnable
    "#,
        "must implement method",
    );
}

#[test]
fn test_trait_method_signature_mismatch_is_error() {
    // Implementing method has wrong return type.
    assert_compiler_error(
        r#"
trait Counter
    fn count() int

class BadCounter implements Counter
    fn count() String
        "oops"
    "#,
        "does not match trait",
    );
}

#[test]
fn test_trait_method_wrong_parameter_count_is_error() {
    // Implementing method has too many parameters.
    assert_compiler_error(
        r#"
trait Adder
    fn add(a int, b int) int

class BadAdder implements Adder
    fn add(a int, b int, c int) int
        a + b + c
    "#,
        "does not match trait",
    );
}

#[test]
fn test_implementing_undefined_trait_is_error() {
    // Referencing a trait that was never defined must be a compile error.
    assert_compiler_error(
        r#"
class Phantom implements NonExistentTrait
    "#,
        "not defined",
    );
}

#[test]
fn test_missing_parent_trait_method_is_error() {
    // Class implements a child trait but omits a method from the parent trait.
    assert_compiler_error(
        r#"
trait Base
    fn base()

trait Extended extends Base
    fn extended()

class Partial implements Extended
    fn extended()
        // forgot fn base()
    "#,
        "must implement method",
    );
}

#[test]
fn test_trait_cannot_be_instantiated_directly() {
    // Traits are not concrete types — instantiating them directly must fail.
    assert_compiler_error(
        r#"
trait Abstract
    fn doIt()

fn main()
    let x = Abstract()
    "#,
        // The error can be about "not a class", "cannot instantiate", or similar.
        "Abstract",
    );
}

// ── Self type in traits ───────────────────────────────────────────────────────

#[test]
fn test_trait_method_uses_self_type() {
    // A trait method that accepts `Self` as a parameter — the implementing class
    // must provide the method with its own type in place of `Self`.
    // Note: `Equatable` is a built-in stdlib trait name; use `SameAs` to avoid
    // the "already defined" clash.
    assert_runs_with_output(
        r#"
use system.io

trait SameAs
    fn same(other Self) bool

class Point implements SameAs
    var x int
    var y int
    fn same(other Point) bool
        self.x == other.x

fn main()
    let a = Point(x: 3, y: 0)
    let b = Point(x: 3, y: 1)
    let c = Point(x: 5, y: 0)
    println(f"{a.same(b)}")
    println(f"{a.same(c)}")
    "#,
        "true\nfalse",
    );
}
