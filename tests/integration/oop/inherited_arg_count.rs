// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests that verify the type checker catches argument-count and type
//! mismatches for inherited constructors, inherited methods, and trait methods.

use super::utils::*;

// ── Inherited constructor: wrong arg count ────────────────────────────────────

#[test]
fn test_inherited_constructor_too_few_args() {
    // Sub has no init; Base.init requires one argument — Sub() must fail.
    assert_compiler_error(
        r#"
class Base
    var x int
    fn init(x int)
        self.x = x

class Sub extends Base

fn main()
    let s = Sub()
    "#,
        "Missing argument for parameter 'x'",
    );
}

#[test]
fn test_inherited_constructor_too_many_args() {
    assert_compiler_error(
        r#"
class Base
    var x int
    fn init(x int)
        self.x = x

class Sub extends Base

fn main()
    let s = Sub(1, 2)
    "#,
        "Too many arguments for 'Sub' constructor",
    );
}

#[test]
fn test_inherited_constructor_wrong_type() {
    assert_compiler_error(
        r#"
class Base
    var x int
    fn init(x int)
        self.x = x

class Sub extends Base

fn main()
    let s = Sub("oops")
    "#,
        "Type mismatch for argument 'x'",
    );
}

#[test]
fn test_inherited_constructor_three_levels_too_few() {
    // Poodle → Dog → Animal, only Animal has init.
    assert_compiler_error(
        r#"
class Animal
    var name String
    fn init(name String)
        self.name = name

class Dog extends Animal

class Poodle extends Dog

fn main()
    let p = Poodle()
    "#,
        "Missing argument for parameter 'name'",
    );
}

#[test]
fn test_inherited_constructor_correct_call_passes() {
    // Passing the right arg to an inherited constructor must type-check fine.
    assert_type_checks(
        r#"
use system.io

class Base
    var x int
    fn init(x int)
        self.x = x

class Sub extends Base

fn main()
    let s = Sub(42)
    println(f"{s.x}")
    "#,
    );
}

#[test]
fn test_subclass_own_init_shadows_parent() {
    // Sub overrides init with different params — its own params are validated,
    // not the parent's.
    assert_compiler_error(
        r#"
class Base
    var x int
    fn init(x int)
        self.x = x

class Sub extends Base
    var y int
    fn init(x int, y int)
        super.init(x)
        self.y = y

fn main()
    let s = Sub(1)
    "#,
        "Missing argument for parameter 'y'",
    );
}

// ── Inherited method: wrong arg count ────────────────────────────────────────

#[test]
fn test_inherited_method_too_few_args() {
    assert_compiler_error(
        r#"
class Animal
    fn move_to(x int, y int)
        ()

class Dog extends Animal

fn main()
    let d = Dog()
    d.move_to(1)
    "#,
        "Missing argument for parameter 'y'",
    );
}

#[test]
fn test_inherited_method_too_many_args() {
    assert_compiler_error(
        r#"
class Animal
    fn speak()
        ()

class Dog extends Animal

fn main()
    let d = Dog()
    d.speak(42)
    "#,
        "Too many positional arguments",
    );
}

#[test]
fn test_inherited_method_wrong_type() {
    assert_compiler_error(
        r#"
class Calculator
    fn add(a int, b int) int
        a + b

class ScientificCalc extends Calculator

fn main()
    let c = ScientificCalc()
    let _ = c.add(1, "two")
    "#,
        "Type mismatch for argument 'b'",
    );
}

#[test]
fn test_inherited_method_correct_call_passes() {
    assert_type_checks(
        r#"
use system.io

class Animal
    fn speak(msg String)
        ()

class Dog extends Animal

fn main()
    let d = Dog()
    d.speak("woof")
    println("ok")
    "#,
    );
}

// ── Trait method: wrong arg count / type ─────────────────────────────────────

#[test]
fn test_trait_method_too_few_args() {
    assert_compiler_error(
        r#"
trait Adder
    fn add(a int, b int) int

class MyAdder implements Adder
    fn add(a int, b int) int
        a + b

fn main()
    let m = MyAdder()
    let _ = m.add(1)
    "#,
        "Missing argument for parameter 'b'",
    );
}

#[test]
fn test_trait_method_too_many_args() {
    assert_compiler_error(
        r#"
trait Greeter
    fn greet()

class Person implements Greeter
    fn greet()
        ()

fn main()
    let p = Person()
    p.greet(42)
    "#,
        "Too many positional arguments",
    );
}

#[test]
fn test_trait_method_wrong_type() {
    assert_compiler_error(
        r#"
trait Scaler
    fn scale(factor int) int

class Box implements Scaler
    var size int = 1
    fn scale(factor int) int
        self.size * factor

fn main()
    let b = Box()
    let _ = b.scale("big")
    "#,
        "Type mismatch for argument 'factor'",
    );
}

#[test]
fn test_trait_method_correct_call_passes() {
    assert_type_checks(
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
    );
}

// ── Trait with default method: wrong arg count ────────────────────────────────

#[test]
fn test_trait_default_method_too_many_args() {
    assert_compiler_error(
        r#"
use system.io

trait Printable
    fn describe() String
        "thing"

class Widget implements Printable

fn main()
    let w = Widget()
    println(w.describe(99))
    "#,
        "Too many positional arguments",
    );
}
