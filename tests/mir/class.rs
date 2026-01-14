// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR tests for class and trait declaration lowering.
//!
//! These tests verify that class and trait declarations are correctly
//! processed through the pipeline.

use miri::pipeline::Pipeline;

fn check_compiles(source: &str) {
    let pipeline = Pipeline::new();
    pipeline.frontend(source).expect("Frontend should succeed");
}

#[test]
fn test_class_declaration_compiles() {
    let source = "
class Animal
    var name String
    ";
    check_compiles(source);
}

#[test]
fn test_class_with_inheritance_compiles() {
    let source = "
class Animal
    var name String

class Dog extends Animal
    var breed String
    ";
    check_compiles(source);
}

#[test]
fn test_trait_declaration_compiles() {
    let source = "
trait Drawable
    fn draw() int
        0
    ";
    check_compiles(source);
}

#[test]
fn test_trait_with_parent_traits_compiles() {
    let source = "
trait Drawable
    fn draw() int
        0

trait Printable
    fn print() int
        0

trait Canvas extends Drawable, Printable
    fn clear() int
        0
    ";
    check_compiles(source);
}

#[test]
fn test_class_implements_trait_compiles() {
    let source = "
trait Drawable
    fn draw() int
        0

class Circle implements Drawable
    var radius float
    ";
    check_compiles(source);
}

#[test]
fn test_generic_class_compiles() {
    let source = "
class Box<T>
    var value T
    ";
    check_compiles(source);
}
