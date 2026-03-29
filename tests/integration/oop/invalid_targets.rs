// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── implements must target a trait ───────────────────────────────────────────

#[test]
fn test_implements_class_is_error() {
    // A class used as an implements target should be rejected.
    assert_compiler_error(
        r#"
class Y
    fn hello()
        println("hello")

class X implements Y
    fn print()
        println("")
    "#,
        "is not a trait",
    );
}

#[test]
fn test_implements_enum_is_error() {
    assert_compiler_error(
        r#"
enum Color
    Red
    Blue

class Palette implements Color
    "#,
        "is not a trait",
    );
}

#[test]
fn test_implements_struct_is_error() {
    assert_compiler_error(
        r#"
struct Point
    x int
    y int

class Plotter implements Point
    "#,
        "is not a trait",
    );
}

#[test]
fn test_implements_type_alias_is_error() {
    assert_compiler_error(
        r#"
type MyInt is int

class Wrapper implements MyInt
    "#,
        "is not a trait",
    );
}

// ── extends in classes must target a class ───────────────────────────────────

#[test]
fn test_extends_trait_in_class_is_error() {
    assert_compiler_error(
        r#"
trait Speakable
    fn speak()

class Dog extends Speakable
    fn speak()
        println("woof")
    "#,
        "is not a class",
    );
}

#[test]
fn test_extends_enum_in_class_is_error() {
    assert_compiler_error(
        r#"
enum Direction
    North
    South

class Compass extends Direction
    "#,
        "is not a class",
    );
}

#[test]
fn test_extends_struct_in_class_is_error() {
    assert_compiler_error(
        r#"
struct Vec2
    x int
    y int

class Physics extends Vec2
    "#,
        "is not a class",
    );
}

#[test]
fn test_extends_type_alias_in_class_is_error() {
    assert_compiler_error(
        r#"
type Num is int

class Counter extends Num
    "#,
        "is not a class",
    );
}

// ── extends in traits must target a trait ─────────────────────────────────────

#[test]
fn test_trait_extends_class_is_error() {
    assert_compiler_error(
        r#"
class Animal
    fn breathe()
        println("breathing")

trait Pet extends Animal
    fn name() String
    "#,
        "is not a trait",
    );
}

#[test]
fn test_trait_extends_enum_is_error() {
    assert_compiler_error(
        r#"
enum Color
    Red
    Blue

trait Colorable extends Color
    fn paint()
    "#,
        "is not a trait",
    );
}

#[test]
fn test_trait_extends_struct_is_error() {
    assert_compiler_error(
        r#"
struct Pos
    x int
    y int

trait Movable extends Pos
    fn move_to(x int, y int)
    "#,
        "is not a trait",
    );
}

#[test]
fn test_trait_extends_type_alias_is_error() {
    assert_compiler_error(
        r#"
type Name is String

trait Named extends Name
    fn get_name() String
    "#,
        "is not a trait",
    );
}
