// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ===== self expression =====

#[test]
fn test_self_inside_class_method() {
    let code = "
class Point
    var x int
    var y int
    fn setX(value int)
        self.x = value
    ";
    type_checker_test(code);
}

#[test]
fn test_self_field_access() {
    let code = "
class Counter
    var count int
    fn increment()
        self.count = self.count + 1
    ";
    type_checker_test(code);
}

#[test]
fn test_self_method_call() {
    let code = "
class Foo
    fn bar() int
        1
    fn baz() int
        self.bar()
    ";
    type_checker_test(code);
}

#[test]
fn test_self_outside_class_error() {
    let code = "
let x = self
    ";
    type_checker_error_test(code, "'self' can only be used inside a class method");
}

#[test]
fn test_self_in_top_level_function_error() {
    let code = "
fn foo()
    self.x
    ";
    type_checker_error_test(code, "'self' can only be used inside a class method");
}

// ===== super expression =====

#[test]
fn test_super_method_call() {
    let code = "
class Animal
    protected fn speak() String
        \"generic sound\"

class Dog extends Animal
    fn speak() String
        super.speak()
    ";
    type_checker_test(code);
}

#[test]
fn test_super_with_base_class() {
    let code = "
class Base
    var value int
    protected fn init(v int)
        self.value = v

class Derived extends Base
    fn init(v int)
        super.init(v)
    ";
    type_checker_test(code);
}

#[test]
fn test_super_outside_class_error() {
    let code = "
let x = super.foo()
    ";
    type_checker_error_test(code, "'super' can only be used inside a class method");
}

#[test]
fn test_super_in_top_level_function_error() {
    let code = "
fn foo()
    super.bar()
    ";
    type_checker_error_test(code, "'super' can only be used inside a class method");
}

#[test]
fn test_super_without_base_class_error() {
    let code = "
class Orphan
    fn foo()
        super.bar()
    ";
    type_checker_error_test(
        code,
        "'super' can only be used in a class that extends another class",
    );
}

// ===== Combined self/super scenarios =====

#[test]
fn test_self_and_super_in_derived_class() {
    let code = "
class Parent
    var name String
    protected fn greet() String
        \"Hello\"

class Child extends Parent
    var age int
    fn greet() String
        super.greet()
    fn setAge(a int)
        self.age = a
    ";
    type_checker_test(code);
}

#[test]
fn test_self_in_trait_method() {
    // Traits can have self references in default implementations
    let code = "
trait Counter
    var count int
    fn increment()
        self.count = self.count + 1
    ";
    // Traits shouldn't allow fields and this would be an error
    type_checker_error_test(code, "Only method declarations are allowed in trait body");
}

#[test]
fn test_self_in_class_init() {
    let code = "
class Person
    var name String
    var age int
    fn init(n String, a int)
        self.name = n
        self.age = a
    ";
    type_checker_test(code);
}
