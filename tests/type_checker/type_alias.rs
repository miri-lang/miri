// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{
    type_checker_error_test, type_checker_expr_type_test, type_checker_test,
    type_checker_vars_type_test,
};
use miri::ast::factory::*;

#[test]
fn test_type_alias_simple() {
    type_checker_test("type MyInt is int\nlet x MyInt = 5");
}

#[test]
fn test_type_alias_as_variable_type() {
    type_checker_vars_type_test(
        "
type MyInt is int
let x MyInt = 5
",
        vec![("x", type_int())],
    );
}

#[test]
fn test_type_alias_as_function_parameter() {
    type_checker_test(
        "
type MyInt is int

fn foo(x MyInt) int
    return x

foo(5)
",
    );
}

#[test]
fn test_type_alias_as_function_return_type() {
    type_checker_expr_type_test(
        "
type MyInt is int

fn foo() MyInt
    return 5

foo()
",
        type_int(),
    );
}

#[test]
fn test_type_alias_in_class_field() {
    type_checker_test(
        "
type MyInt is int

class Foo
    var x MyInt
",
    );
}

#[test]
fn test_type_alias_in_struct_field() {
    type_checker_test(
        "
type MyInt is int

struct Point
    x MyInt
    y MyInt

let p = Point(1, 2)
",
    );
}

#[test]
fn test_type_alias_with_nullable() {
    type_checker_test(
        "
type OptionalInt is int?
var x OptionalInt = 5
x = None
",
    );
}

#[test]
fn test_type_alias_with_list() {
    type_checker_test(
        "
type IntList is [int]
let list IntList = [1, 2, 3]
",
    );
}

#[test]
fn test_type_alias_with_map() {
    type_checker_test(
        "
type StringIntMap is {string: int}
let map StringIntMap = {\"a\": 1, \"b\": 2}
",
    );
}

#[test]
fn test_type_alias_with_tuple() {
    type_checker_test(
        "
type Pair is (int, string)
let p Pair = (1, \"hello\")
",
    );
}

#[test]
fn test_type_alias_extends_class() {
    // type X extends Y cannot be used on an already-defined type
    type_checker_error_test(
        "
class Animal
    var name String

class Dog
    var name String
    var breed String

type Dog extends Animal
",
        "Type 'Dog' is already defined",
    );
}

#[test]
fn test_type_alias_implements_trait() {
    // type X implements Y cannot be used on an already-defined type
    type_checker_error_test(
        "
trait Drawable
    fn draw() int
        0

class Circle
    var radius float
    fn draw() int
        1

type Circle implements Drawable
",
        "Type 'Circle' is already defined",
    );
}

#[test]
fn test_type_alias_generic() {
    type_checker_test(
        "
type Optional<T> is T?
var x Optional<int> = 5
x = None
",
    );
}

#[test]
fn test_type_alias_generic_in_function() {
    // Generic type alias used as function parameter type
    type_checker_test(
        "
type Optional<T> is T?

fn process(x Optional<int>) int
    5

process(42)
",
    );
}

#[test]
fn test_type_alias_complex_nested() {
    type_checker_test(
        "
type UserMap is {string: [int]}
let map UserMap = {\"scores\": [1, 2, 3]}
",
    );
}

#[test]
fn test_type_alias_undefined_target() {
    type_checker_error_test("type MyInt is Unknown", "Unknown type");
}

#[test]
fn test_type_alias_type_mismatch() {
    type_checker_error_test(
        "
type MyInt is int
let x MyInt = \"not an int\"
",
        "Type mismatch",
    );
}

#[test]
fn test_type_alias_extends_undefined() {
    type_checker_error_test("type X extends Unknown", "Unknown type 'Unknown'");
}

#[test]
fn test_type_alias_implements_undefined() {
    type_checker_error_test("type X implements Unknown", "Unknown type 'Unknown'");
}

#[test]
fn test_type_parameter_unconstrained() {
    // Generics are declared directly on functions, not via standalone type statements
    type_checker_test(
        "
fn identity<T>(x T) T
    return x

identity(5)
",
    );
}

#[test]
fn test_type_parameter_constrained_extends() {
    // TODO: should this be allowed?
    type_checker_test(
        "
class Animal
    var name String

type T extends Animal

class Container<T extends Animal>
    var item T
",
    );
}

#[test]
fn test_type_alias_chain() {
    type_checker_test(
        "
type A is int
type B is A
let x B = 5
",
    );
}

#[test]
fn test_type_alias_in_method_parameter() {
    type_checker_test(
        "
type MyInt is int

fn add(a MyInt, b MyInt) MyInt
    a + b

add(1, 2)
",
    );
}

#[test]
fn test_multiple_type_declarations_incomplete() {
    // Incomplete type declarations without is/extends/implements are not allowed
    type_checker_error_test(
        "
type A, B, C
",
        "Incomplete type declaration",
    );
}

#[test]
fn test_protected_type_alias() {
    // TODO: types should be just private or public. Protected is not needed.
    type_checker_test(
        "
protected type InternalInt is int
let x InternalInt = 5
",
    );
}

#[test]
fn test_type_alias_deeply_nested() {
    type_checker_test(
        "
type IntList is [int]
type IntListList is [IntList]
type IntListListList is [IntListList]

let deep IntListListList = [[[1, 2], [3, 4]]]
",
    );
}

#[test]
fn test_type_alias_in_match() {
    type_checker_test(
        "
type MyInt is int

let x MyInt = 5
match x
    1: \"one\"
    2: \"two\"
    _: \"other\"
",
    );
}

#[test]
fn test_type_alias_multiple_usages() {
    type_checker_test(
        "
type Point is (int, int)

let p1 Point = (1, 2)
let p2 Point = (3, 4)
let p3 Point = (5, 6)
",
    );
}

#[test]
fn test_type_alias_with_complex_map() {
    type_checker_test(
        "
type UserScores is {string: [int]}
let scores UserScores = {\"alice\": [100, 95], \"bob\": [80, 85]}
",
    );
}

#[test]
fn test_type_alias_in_for_loop() {
    type_checker_test(
        "
type Numbers is [int]
let nums Numbers = [1, 2, 3]
for n in nums
    let x = n * 2
",
    );
}

#[test]
fn test_type_alias_function_type() {
    type_checker_test(
        "
type Transformer is fn(int) int

let double Transformer = fn(x int): x * 2
double(5)
",
    );
}
