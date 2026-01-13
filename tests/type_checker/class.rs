// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::type_checker::utils::{check_error, check_success};

// ===== Class Declaration =====

#[test]
fn test_class_declaration_basic() {
    let code = "
class Animal
    var name String
    ";
    check_success(code);
}

#[test]
fn test_class_declaration_with_field_types() {
    let code = "
class Point
    var x int
    var y int
    ";
    check_success(code);
}

#[test]
fn test_class_duplicate_name() {
    let code = "
class Point
    var x int

class Point
    var y int
    ";
    check_error(code, "already defined");
}

// ===== Class Inheritance =====

#[test]
fn test_class_extends() {
    let code = "
class Animal
    var name String

class Dog extends Animal
    var breed String
    ";
    check_success(code);
}

#[test]
fn test_class_extends_undefined() {
    let code = "
class Dog extends Animal
    var breed String
    ";
    check_error(code, "not defined");
}

// ===== Class Implements =====

#[test]
fn test_class_implements_trait() {
    let code = "
trait Drawable
    fn draw() int
        0

class Circle implements Drawable
    var radius float
    ";
    check_success(code);
}

#[test]
fn test_class_implements_undefined_trait() {
    let code = "
class Circle implements Drawable
    var radius float
    ";
    check_error(code, "not defined");
}

// ===== Generic Classes =====

#[test]
fn test_class_generic() {
    let code = "
class Box<T>
    var value T
    ";
    check_success(code);
}

// ===== Traits =====

#[test]
fn test_trait_declaration_basic() {
    let code = "
trait Drawable
    fn draw() int
        0
    ";
    check_success(code);
}

#[test]
fn test_trait_extends() {
    let code = "
trait Drawable
    fn draw() int
        0

trait Resizable
    fn resize(width int, height int) int
        0

trait Shape extends Drawable, Resizable
    fn area() float
        0.0
    ";
    check_success(code);
}

#[test]
fn test_trait_extends_undefined() {
    let code = "
trait Shape extends Unknown
    fn area() float
        0.0
    ";
    check_error(code, "not defined");
}

#[test]
fn test_trait_duplicate_name() {
    let code = "
trait Drawable
    fn draw() int
        0

trait Drawable
    fn render() int
        0
    ";
    check_error(code, "already defined");
}

// ===== Combined Class/Trait =====

#[test]
fn test_class_extends_and_implements() {
    let code = "
trait Serializable
    fn serialize() string
        \"\"

class Animal
    var name String

class Dog extends Animal implements Serializable
    var breed String
    ";
    check_success(code);
}

#[test]
fn test_class_implements_multiple_traits() {
    let code = "
trait Drawable
    fn draw() int
        0

trait Printable
    fn print() int
        0

class Shape implements Drawable, Printable
    var id int
    ";
    check_success(code);
}
