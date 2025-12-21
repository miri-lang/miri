// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::{factory, Type};

#[test]
fn test_list_literal_int() {
    check_expr_type("[1, 2, 3]", Type::List(Box::new(factory::typ(Type::Int))));
}

#[test]
fn test_list_literal_string() {
    check_expr_type(
        "[\"a\", \"b\"]",
        Type::List(Box::new(factory::typ(Type::String))),
    );
}

#[test]
fn test_list_literal_mixed_error() {
    check_error("[1, \"a\"]", "List elements must have the same type");
}

#[test]
fn test_list_indexing() {
    check_expr_type("[1, 2, 3][0]", Type::Int);
}

#[test]
fn test_list_indexing_invalid_index_type() {
    check_error("[1, 2, 3][\"a\"]", "List index must be an integer");
}

#[test]
fn test_list_indexing_on_non_list() {
    check_error("1[0]", "Type Int is not indexable");
}

#[test]
fn test_list_indexing_variable() {
    check_expr_type(
        "
let i = 0
[1, 2, 3][i]
",
        Type::Int,
    );
}

#[test]
fn test_list_indexing_function_call() {
    check_expr_type(
        "
fn get_index() int
    return 0

[1, 2, 3][get_index()]
",
        Type::Int,
    );
}

#[test]
fn test_list_indexing_variable_type_mismatch() {
    check_error(
        "
let i = \"0\"
[1, 2, 3][i]
",
        "List index must be an integer",
    );
}

#[test]
fn test_empty_list() {
    check_expr_type("[]", Type::List(Box::new(factory::typ(Type::Void))));
}

#[test]
fn test_nested_list() {
    check_expr_type(
        "[[1, 2], [3, 4]]",
        Type::List(Box::new(factory::typ(Type::List(Box::new(factory::typ(
            Type::Int,
        )))))),
    );
}

#[test]
fn test_nested_list_mixed_error() {
    check_error("[[1], [\"a\"]]", "List elements must have the same type");
}

#[test]
fn test_list_assignment_exact() {
    check_success(
        "
let l [int] = [1, 2, 3]
",
    );
}

#[test]
fn test_list_assignment_mismatch_type() {
    check_error(
        "
let l [string] = [1, 2, 3]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_invariant() {
    // Lists are invariant, but list literals should be inferred based on the target type if possible
    check_success(
        "
let l [i16] = [1]
",
    );
}

#[test]
fn test_list_assignment_overflow() {
    check_error(
        "
let l [i8] = [1000]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_signed_unsigned_mismatch() {
    check_error(
        "
let l [u8] = [-1]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_i8_overflow() {
    check_error(
        "
let l [i8] = [128]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_mutability() {
    check_success(
        "
var l = [1, 2, 3]
l[0] = 4
",
    );
}

#[test]
fn test_list_mutability_type_mismatch() {
    check_error(
        "
var l = [1, 2, 3]
l[0] = \"a\"
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_list_of_functions() {
    check_success(
        "
let l = [fn(x int): x, fn(x int): x * 2]
l[0](1)
",
    );
}

#[test]
fn test_list_of_functions_mismatch() {
    check_error(
        "
let l = [fn(x int): x, fn(x string): x]
",
        "List elements must have the same type",
    );
}

#[test]
fn test_list_assignment_to_immutable_index() {
    check_error(
        "
let l = [1, 2, 3]
l[0] = 4
",
        "Cannot assign to element of immutable variable",
    );
}

#[test]
fn test_mutable_list_assignment() {
    check_success(
        "
var l = [1, 2, 3]
l[0] = 4
",
    );
}
