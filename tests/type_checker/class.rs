// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_class_declaration_basic() {
    let code = "
class Animal
    var name String
    ";
    type_checker_test(code);
}

#[test]
fn test_class_declaration_with_field_types() {
    let code = "
class Point
    var x int
    var y int
    ";
    type_checker_test(code);
}

#[test]
fn test_class_duplicate_name() {
    let code = "
class Point
    var x int

class Point
    var y int
    ";
    type_checker_error_test(code, "already defined");
}

#[test]
fn test_class_extends() {
    let code = "
class Animal
    var name String

class Dog extends Animal
    var breed String
    ";
    type_checker_test(code);
}

#[test]
fn test_class_extends_undefined() {
    let code = "
class Dog extends Animal
    var breed String
    ";
    type_checker_error_test(code, "not defined");
}

#[test]
fn test_class_implements_trait() {
    let code = "
trait Drawable
    fn draw() int
        0

class Circle implements Drawable
    var radius float
    ";
    type_checker_test(code);
}

#[test]
fn test_class_implements_undefined_trait() {
    let code = "
class Circle implements Drawable
    var radius float
    ";
    type_checker_error_test(code, "not defined");
}

#[test]
fn test_class_generic() {
    let code = "
class Box<T>
    var value T
    ";
    type_checker_test(code);
}

#[test]
fn test_trait_declaration_basic() {
    let code = "
trait Drawable
    fn draw() int
        0
    ";
    type_checker_test(code);
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
    type_checker_test(code);
}

#[test]
fn test_trait_extends_undefined() {
    let code = "
trait Shape extends Unknown
    fn area() float
        0.0
    ";
    type_checker_error_test(code, "not defined");
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
    type_checker_error_test(code, "already defined");
}

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
    type_checker_test(code);
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
    type_checker_test(code);
}

#[test]
fn test_abstract_class_with_abstract_method() {
    let code = "
abstract class Shape
    fn area() float
    ";
    type_checker_test(code);
}

#[test]
fn test_abstract_class_with_concrete_method() {
    let code = "
abstract class Shape
    fn describe() string
        \"A shape\"
    ";
    type_checker_test(code);
}

#[test]
fn test_abstract_class_mixed_methods() {
    let code = "
abstract class Shape
    fn area() float
    fn describe() string
        \"A shape\"
    ";
    type_checker_test(code);
}

#[test]
fn test_non_abstract_class_with_abstract_method_error() {
    let code = "
class Shape
    fn area() float

0
    ";
    type_checker_error_test(
        code,
        "Non-abstract class 'Shape' cannot have abstract method",
    );
}

#[test]
fn test_concrete_class_extends_abstract_implements_all() {
    let code = "
abstract class Shape
    fn area() float

class Circle extends Shape
    var radius float
    fn area() float
        3.14 * self.radius * self.radius
    ";
    type_checker_test(code);
}

#[test]
fn test_concrete_class_extends_abstract_missing_implementation() {
    let code = "
abstract class Shape
    fn area() float

class Circle extends Shape
    var radius float
    ";
    type_checker_error_test(
        code,
        "must implement abstract method 'area' from class 'Shape'",
    );
}

#[test]
fn test_abstract_class_extends_abstract_no_implementation_required() {
    let code = "
abstract class Shape
    fn area() float

abstract class Polygon extends Shape
    fn perimeter() float
    ";
    type_checker_test(code);
}

#[test]
fn test_concrete_class_extends_abstract_chain() {
    let code = "
abstract class Shape
    fn area() float

abstract class Polygon extends Shape
    fn perimeter() float

class Rectangle extends Polygon
    var width float
    var height float
    fn area() float
        self.width * self.height
    fn perimeter() float
        2.0 * (self.width + self.height)
    ";
    type_checker_test(code);
}

#[test]
fn test_abstract_class_instantiation_error() {
    let code = "
abstract class Shape
    fn area() float

let s = Shape()
    ";
    type_checker_error_test(code, "Cannot instantiate abstract class");
}

#[test]
fn test_class_implements_trait_all_methods() {
    let code = "
trait Drawable
    fn draw() int

class Circle implements Drawable
    var radius float
    fn draw() int
        1
    ";
    type_checker_test(code);
}

#[test]
fn test_class_implements_trait_missing_method() {
    let code = "
trait Drawable
    fn draw() int

class Circle implements Drawable
    var radius float
    ";
    type_checker_error_test(code, "must implement method 'draw' from trait 'Drawable'");
}

#[test]
fn test_class_implements_trait_with_default_method() {
    let code = "
trait Drawable
    fn draw() int
        0

class Circle implements Drawable
    var radius float
    ";
    // Default implementation means the class doesn't need to implement it
    type_checker_test(code);
}

#[test]
fn test_class_implements_trait_wrong_return_type() {
    let code = "
trait Drawable
    fn draw() int

class Circle implements Drawable
    var radius float
    fn draw() string
        \"circle\"
    ";
    type_checker_error_test(code, "does not match trait 'Drawable' signature");
}

#[test]
fn test_class_implements_trait_wrong_param_count() {
    let code = "
trait Drawable
    fn draw(x int) int

class Circle implements Drawable
    var radius float
    fn draw() int
        1
    ";
    type_checker_error_test(code, "does not match trait 'Drawable' signature");
}

#[test]
fn test_class_implements_multiple_traits_success() {
    let code = "
trait Drawable
    fn draw() int

trait Printable
    fn print() string

class Shape implements Drawable, Printable
    var id int
    fn draw() int
        1
    fn print() string
        \"shape\"
    ";
    type_checker_test(code);
}

#[test]
fn test_class_implements_multiple_traits_missing_one() {
    let code = "
trait Drawable
    fn draw() int

trait Printable
    fn print() string

class Shape implements Drawable, Printable
    var id int
    fn draw() int
        1
    ";
    type_checker_error_test(code, "must implement method 'print' from trait 'Printable'");
}

#[test]
fn test_multi_level_inheritance_chain() {
    let code = "
class Animal
    var name String

class Mammal extends Animal
    var legs int

class Dog extends Mammal
    var breed String
    ";
    type_checker_test(code);
}

#[test]
fn test_abstract_method_through_chain() {
    let code = "
abstract class Shape
    fn area() float

abstract class Polygon extends Shape
    fn sides() int

class Triangle extends Polygon
    var base float
    var height float
    fn area() float
        0.5 * self.base * self.height
    fn sides() int
        3
    ";
    type_checker_test(code);
}

#[test]
fn test_abstract_method_missing_from_grandparent() {
    let code = "
abstract class Shape
    fn area() float

class Polygon extends Shape
    var name String

class Triangle extends Polygon
    var base float
    ";
    // Triangle should error because it doesn't implement area() from Shape
    type_checker_error_test(
        code,
        "must implement abstract method 'area' from class 'Shape'",
    );
}

#[test]
fn test_circular_inheritance_direct() {
    // Note: This test may not work because A needs to be defined before B can extend it
    // But we test what we can - classes extending themselves indirectly
    let code = "
class A extends B
    var x int

class B extends A
    var y int
    ";
    type_checker_error_test(code, "not defined");
}

#[test]
fn test_class_deep_inheritance_chain() {
    type_checker_test(
        "
class A
    var a int
class B extends A
    var b int
class C extends B
    var c int
class D extends C
    var d int
class E extends D
    var e int

let e = E()
",
    );
}

#[test]
fn test_class_many_fields() {
    type_checker_test(
        "
class BigClass
    var a int
    var b int
    var c int
    var d int
    var e int
    var f int
    var g int
    var h int
    var i int
    var j int

let obj = BigClass()
",
    );
}

#[test]
fn test_class_implements_many_traits() {
    type_checker_test(
        "
trait A
    fn a() int: 0
trait B
    fn b() int: 0
trait C
    fn c() int: 0

class Impl implements A, B, C
    var x int
",
    );
}

#[test]
fn test_abstract_chain_deep() {
    type_checker_test(
        "
abstract class A
    fn method_a() int

abstract class B extends A
    fn method_b() int

abstract class C extends B
    fn method_c() int

class Concrete extends C
    fn method_a() int: 1
    fn method_b() int: 2
    fn method_c() int: 3
",
    );
}

#[test]
fn test_class_with_generic_field() {
    type_checker_test(
        "
struct Box<T>
    value T

class Container
    var box Box<int>
",
    );
}

#[test]
fn test_trait_with_complex_signature() {
    type_checker_test(
        "
trait Processor
    fn process(items [int], factor int) [int]
        items

class SimpleProcessor implements Processor
    var name String
",
    );
}

#[test]
fn test_class_self_reference_type() {
    // TODO: Feature not implemented - class self-reference as field type
    type_checker_error_test(
        "
class Node
    var value int
    var next Node?
",
        "Unknown type",
    );
}

#[test]
fn test_class_inheritance_method_override() {
    type_checker_test(
        "
class Animal
    fn speak() string
        \"generic sound\"

class Dog extends Animal
    fn speak() string
        \"woof\"

let d = Dog()
",
    );
}
