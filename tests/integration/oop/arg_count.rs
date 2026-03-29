// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests that verify the type checker catches argument-count and type
//! mismatches when invoking class constructors (`init` methods).

use super::utils::*;

// ── Too few arguments ─────────────────────────────────────────────────────────

#[test]
fn test_constructor_too_few_args() {
    assert_compiler_error(
        r#"
class User
    var name String
    var age u8
    var role String
    fn init(name String, age u8, role String)
        self.name = name
        self.age  = age
        self.role = role

fn main()
    let u = User("Alice", 16)
    "#,
        "Missing argument for parameter 'role'",
    );
}

#[test]
fn test_constructor_zero_args_when_one_required() {
    assert_compiler_error(
        r#"
class Counter
    var n int
    fn init(n int)
        self.n = n

fn main()
    let c = Counter()
    "#,
        "Missing argument for parameter 'n'",
    );
}

// ── Too many arguments ────────────────────────────────────────────────────────

#[test]
fn test_constructor_too_many_positional_args() {
    assert_compiler_error(
        r#"
class Point
    var x int
    var y int
    fn init(x int, y int)
        self.x = x
        self.y = y

fn main()
    let p = Point(1, 2, 3)
    "#,
        "Too many arguments for 'Point' constructor",
    );
}

#[test]
fn test_constructor_too_many_with_no_params() {
    assert_compiler_error(
        r#"
class Empty
    fn init()
        ()

fn main()
    let e = Empty(42)
    "#,
        "Too many arguments for 'Empty' constructor",
    );
}

// ── Wrong argument types ──────────────────────────────────────────────────────

#[test]
fn test_constructor_wrong_arg_type() {
    assert_compiler_error(
        r#"
class Box
    var value int
    fn init(value int)
        self.value = value

fn main()
    let b = Box("hello")
    "#,
        "Type mismatch for argument 'value'",
    );
}

#[test]
fn test_constructor_wrong_second_arg_type() {
    assert_compiler_error(
        r#"
class Pair
    var a int
    var b String
    fn init(a int, b String)
        self.a = a
        self.b = b

fn main()
    let p = Pair(1, 2)
    "#,
        "Type mismatch for argument 'b'",
    );
}

// ── Unknown named argument ────────────────────────────────────────────────────

#[test]
fn test_constructor_unknown_named_arg() {
    assert_compiler_error(
        r#"
class Dog
    var name String
    fn init(name String)
        self.name = name

fn main()
    let d = Dog(breed: "Lab")
    "#,
        "Unknown argument 'breed'",
    );
}

// ── Valid calls must still compile ───────────────────────────────────────────

#[test]
fn test_constructor_correct_positional() {
    assert_type_checks(
        r#"
use system.io

class Pt
    var x int
    var y int
    fn init(x int, y int)
        self.x = x
        self.y = y

fn main()
    let p = Pt(3, 4)
    println(f"{p.x}")
    "#,
    );
}

#[test]
fn test_constructor_correct_named_args() {
    assert_type_checks(
        r#"
use system.io

class Rect
    var w int
    var h int
    fn init(w int, h int)
        self.w = w
        self.h = h

fn main()
    let r = Rect(h: 5, w: 10)
    println(f"{r.w}")
    "#,
    );
}

#[test]
fn test_constructor_no_init_field_style_still_works() {
    // Classes without an explicit `init` can still be constructed with
    // named field arguments — this must not regress.
    assert_type_checks(
        r#"
use system.io

class Coord
    var x int
    var y int

fn main()
    let c = Coord(x: 1, y: 2)
    println(f"{c.x}")
    "#,
    );
}
