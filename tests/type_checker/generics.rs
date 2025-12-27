// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::{check_error, check_expr_type, check_success};
use miri::ast::factory::*;

#[test]
fn test_generic_implements_constraint_variable() {
    let source = "
struct Interface
    x int

struct Implementation
    x int
    y string

struct Container<T implements Interface>
    val T

let c Container<Implementation>
    ";
    check_success(source);
}

#[test]
fn test_generic_implements_constraint_fail() {
    let source = "
struct Interface
    x int

struct BadImpl
    y string

struct Container<T implements Interface>
    val T

let c Container<BadImpl>
    ";
    check_error(source, "does not satisfy constraint");
}

#[test]
fn test_generic_struct_instantiation() {
    let source = "
struct Box<T>
    value T

let b Box<int>
    ";
    check_success(source);
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
    check_expr_type(source, type_int());
}

#[test]
fn test_generic_struct_field_type_mismatch() {
    let source = "
struct Box<T>
    value T

var b Box<int>
b.value = \"string\"
    ";
    check_error(source, "Type mismatch");
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
    check_expr_type(source, type_int());
}

#[test]
fn test_multiple_generic_params() {
    let source = "
struct Pair<K, V>
    key K
    value V

var p Pair<string, int>
p.key = \"key\"
p.value = 10
p.value
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_generic_argument_count_mismatch() {
    let source = "
struct Box<T>
    value T

let b Box<int, string>
    ";
    check_error(source, "Generic argument count mismatch");
}

#[test]
fn test_generic_argument_count_mismatch_less() {
    let source = "
struct Pair<K, V>
    key K
    value V

let p Pair<int>
    ";
    check_error(source, "Generic argument count mismatch");
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
    check_expr_type(source, type_int());
}

#[test]
fn test_generic_struct_with_map() {
    let source = "
struct MapContainer<K, V>
    items {K: V}

var c MapContainer<string, int>
c.items = {\"a\": 1}
c.items[\"a\"]
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_generic_parameter_shadowing() {
    let source = "
struct Outer<T>
    val T

struct Inner<T>
    val T

let o Outer<int>
let i Inner<string>
    ";
    check_success(source);
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
    check_expr_type(source, type_int());
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
    check_expr_type(source, type_int());
}

#[test]
fn test_generic_struct_inference() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
";
    check_success(source);
}

#[test]
fn test_generic_struct_inference_mismatch() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
let s string = b.value
";
    check_error(source, "Type mismatch");
}

#[test]
fn test_generic_struct_inference_correct_type() {
    let source = "
struct Box<T>
    value T

let b = Box(1)
let i int = b.value
";
    check_success(source);
}
