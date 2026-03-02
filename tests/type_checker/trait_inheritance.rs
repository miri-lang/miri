// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ===== Trait Inheritance =====

#[test]
fn test_class_implements_parent_trait_method() {
    // When a class implements a trait that extends another trait,
    // it must provide implementations for methods from both traits
    let code = "
trait Drawable
    fn draw() int

trait Interactive extends Drawable
    fn click() int

class Button implements Interactive
    fn draw() int
        1
    fn click() int
        2
    ";
    type_checker_test(code);
}

#[test]
fn test_class_implements_trait_missing_parent_method_error() {
    // Missing method from parent trait should be an error
    let code = "
trait Drawable
    fn draw() int

trait Interactive extends Drawable
    fn click() int

class Button implements Interactive
    fn click() int
        1
    ";
    // Should fail because draw() from Drawable is not implemented
    type_checker_error_test(code, "must implement method 'draw'");
}

#[test]
fn test_class_implements_multi_level_trait_hierarchy() {
    // Three levels of trait hierarchy
    let code = "
trait Base
    fn base() int

trait Middle extends Base
    fn middle() int

trait Top extends Middle
    fn top() int

class Widget implements Top
    fn base() int
        1
    fn middle() int
        2
    fn top() int
        3
    ";
    type_checker_test(code);
}

#[test]
fn test_class_implements_trait_with_default_from_parent() {
    // Parent trait has default implementation
    let code = "
trait Drawable
    fn draw() int
        0

trait Interactive extends Drawable
    fn click() int

class Button implements Interactive
    fn click() int
        1
    ";
    // draw() has a default, so this should succeed
    type_checker_test(code);
}

#[test]
fn test_trait_extends_multiple_parents() {
    // Trait extends multiple parent traits
    let code = "
trait Readable
    fn read() String

trait Writable
    fn write(data String) int

trait ReadWrite extends Readable, Writable
    fn flush() int

class File implements ReadWrite
    fn read() String
        \"data\"
    fn write(data String) int
        1
    fn flush() int
        1
    ";
    type_checker_test(code);
}

#[test]
fn test_trait_extends_multiple_missing_one_error() {
    // Missing method from one parent trait
    let code = "
trait Readable
    fn read() String

trait Writable
    fn write(data String) int

trait ReadWrite extends Readable, Writable
    fn flush() int

class File implements ReadWrite
    fn read() String
        \"data\"
    fn flush() int
        1
    ";
    type_checker_error_test(code, "must implement method 'write'");
}
