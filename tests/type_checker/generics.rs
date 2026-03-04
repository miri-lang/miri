// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_expr_type_test, type_checker_test};
use miri::ast::factory::*;

#[test]
fn test_generic_implements_constraint_variable() {
    let source = "
struct Interface
    x int

struct Implementation
    x int
    y String

struct Container<T implements Interface>
    val T

let c Container<Implementation>
    ";
    type_checker_test(source);
}

#[test]
fn test_generic_implements_constraint_fail() {
    let source = "
struct Interface
    x int

struct BadImpl
    y String

struct Container<T implements Interface>
    val T

let c Container<BadImpl>
    ";
    type_checker_error_test(source, "does not satisfy constraint");
}

#[test]
fn test_generic_struct_instantiation() {
    let source = "
struct Box<T>
    value T

let b Box<int>
    ";
    type_checker_test(source);
}

#[test]
fn test_generic_struct_field_access() {
    let source = "
struct Box<T>
    value T

var b Box<int>
b.value = 10
b.value
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_generic_struct_field_type_mismatch() {
    let source = "
struct Box<T>
    value T

var b Box<int>
b.value = \"string\"
    ";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_nested_generic_structs() {
    let source = "
struct Box<T>
    value T

var b Box<Box<int>>
b.value.value = 10
b.value.value
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_multiple_generic_params() {
    let source = "
struct Pair<K, V>
    key K
    value V

var p Pair<String, int>
p.key = \"key\"
p.value = 10
p.value
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_generic_argument_count_mismatch() {
    let source = "
struct Box<T>
    value T

let b Box<int, String>
    ";
    type_checker_error_test(source, "Generic argument count mismatch");
}

#[test]
fn test_generic_argument_count_mismatch_less() {
    let source = "
struct Pair<K, V>
    key K
    value V

let p Pair<int>
    ";
    type_checker_error_test(source, "Generic argument count mismatch");
}

#[test]
fn test_generic_struct_with_list() {
    let source = "
struct ListContainer<T>
    items [T]

var c ListContainer<int>
c.items = [1, 2, 3]
c.items[0]
    ";
    type_checker_error_test(source, "Type mismatch in assignment");
}

#[test]
fn test_generic_struct_with_map() {
    let source = "
struct MapContainer<K, V>
    items {K: V}

var c MapContainer<String, int>
c.items = {\"a\": 1}
c.items[\"a\"]
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_generic_parameter_shadowing() {
    let source = "
struct Outer<T>
    val T

struct Inner<T>
    val T

let o Outer<int>
let i Inner<String>
    ";
    type_checker_test(source);
}

#[test]
fn test_generic_parameter_used_in_method() {
    let source = "
struct Box<T>
    value T

fn unbox<T>(b Box<T>) T
    return b.value

var b Box<int>
b.value = 10
unbox(b)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_generic_type_inference_in_variable_declaration() {
    let source = "
struct Box<T>
    value T

let b Box<int>
let x = b.value
x
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_generic_struct_inference() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
";
    type_checker_test(source);
}

#[test]
fn test_generic_struct_inference_mismatch() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
let s String = b.value
";
    type_checker_error_test(source, "Type mismatch");
}

#[test]
fn test_generic_struct_inference_correct_type() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
let i int = b.value
";
    type_checker_test(source);
}

#[test]
fn test_generic_class_with_extends_constraint() {
    let source = "
class Animal
    var name String

class Dog extends Animal
    var breed String

class Container<T extends Animal>
    var item T

let c Container<Dog>
";
    type_checker_test(source);
}

#[test]
fn test_generic_class_with_extends_constraint_fail() {
    let source = "
class Animal
    var name String

class Robot
    var model String

class Container<T extends Animal>
    var item T

let c Container<Robot>
";
    type_checker_error_test(source, "does not satisfy constraint");
}

#[test]
fn test_generic_class_with_trait_constraint() {
    let source = "
trait Drawable
    fn draw() int

class Circle implements Drawable
    fn draw() int
        1

class Canvas<T implements Drawable>
    var item T
";
    type_checker_test(source);
}

#[test]
fn test_generic_multiple_params() {
    type_checker_test(
        "
struct Triple<A, B, C>
    first A
    second B
    third C

let t Triple<int, String, bool>
",
    );
}

#[test]
fn test_generic_deeply_nested() {
    type_checker_test(
        "
struct Box<T>
    value T

let nested Box<Box<Box<int>>>
",
    );
}

#[test]
fn test_generic_function_chain() {
    type_checker_error_test(
        "
fn wrap<T>(x T) [T]
    return [x]

wrap(wrap(wrap(1)))[0][0]
",
        "Invalid return type",
    );
}

#[test]
fn test_generic_with_nullable() {
    type_checker_test(
        "
struct MaybeBox<T>
    value T?

let b MaybeBox<int>
",
    );
}

#[test]
fn test_generic_function_multiple_params() {
    // TODO: Feature not implemented - generic tuple return types
    type_checker_error_test(
        "
fn pair<A, B>(a A, b B) (A, B)
    return (a, b)

pair(1, \"hello\")
",
        "Invalid return type",
    );
}

#[test]
fn test_generic_in_list_of_generics() {
    type_checker_error_test(
        "
struct Box<T>
    value T

let boxes [Box<int>] = [Box(1), Box(2), Box(3)]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_generic_map_key_value() {
    type_checker_test(
        "
struct Pair<K, V>
    key K
    value V

let p = Pair(\"name\", 42)
",
    );
}

#[test]
fn test_generic_with_complex_inner_type() {
    type_checker_test(
        "
struct Container<T>
    items [T]

let c Container<{String: int}>
",
    );
}
