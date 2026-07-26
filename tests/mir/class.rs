// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_frontend_succeeds;

#[test]
fn test_class_declaration_compiles() {
    mir_frontend_succeeds(
        "
class Animal
    var name String
    ",
    );
}

#[test]
fn test_class_with_inheritance_compiles() {
    mir_frontend_succeeds(
        "
class Animal
    var name String

class Dog extends Animal
    var breed String
    ",
    );
}

#[test]
fn test_trait_declaration_compiles() {
    mir_frontend_succeeds(
        "
trait Drawable
    fn draw() int
        0
    ",
    );
}

#[test]
fn test_trait_with_parent_traits_compiles() {
    mir_frontend_succeeds(
        "
trait Drawable
    fn draw() int
        0

trait Printable
    fn print() int
        0

trait Canvas extends Drawable, Printable
    fn clear() int
        0
    ",
    );
}

#[test]
fn test_class_implements_trait_compiles() {
    mir_frontend_succeeds(
        "
trait Drawable
    fn draw() int
        0

class Circle implements Drawable
    var radius float
    ",
    );
}

#[test]
fn test_generic_class_compiles() {
    mir_frontend_succeeds(
        "
class Box<T>
    var value T
    ",
    );
}
