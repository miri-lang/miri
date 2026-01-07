// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_list_literal_int() {
    check_expr_type("[1, 2, 3]", type_list(type_int()));
}

#[test]
fn test_list_variable_definitions() {
    check_vars_type(
        "
        let l1 [int] = [10, 20, 30]
        let l2 list<string> = [\"a\", \"b\", \"c\"]
        let l3 list<i128> = [1, 2, 3]
        let l4 list<float> = [1.1, 2.2, 3.3]
        let l5 list<f64> = [1.5, 2.5, 3.5]
",
        vec![
            ("l1", type_list(type_int())),
            ("l2", type_list(type_string())),
            ("l3", type_list(type_i128())),
            ("l4", type_list(type_float())),
            ("l5", type_list(type_f64())),
        ],
    )
}

#[test]
fn test_list_literal_string() {
    check_expr_type("[\"a\", \"b\"]", type_list(type_string()));
}

#[test]
fn test_list_literal_mixed_error() {
    check_error("[1, \"a\"]", "List elements must have the same type");
}

#[test]
fn test_list_indexing() {
    check_expr_type("[1, 2, 3][0]", type_int());
}

#[test]
fn test_list_indexing_invalid_index_type() {
    check_error("[1, 2, 3][\"a\"]", "List index must be an integer");
}

#[test]
fn test_list_indexing_on_non_list() {
    check_error("1[0]", "Type int is not indexable");
}

#[test]
fn test_list_indexing_variable() {
    check_expr_type(
        "
let i = 0
[1, 2, 3][i]
",
        type_int(),
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
        type_int(),
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
    check_expr_type("[]", type_list(type_void()));
}

#[test]
fn test_empty_list_with_specified_types() {
    check_vars_type(
        "
    let l1 [string] = []
",
        vec![("l1", type_list(type_string()))],
    );
}

#[test]
fn test_empty_list_with_specified_types_named() {
    check_vars_type(
        "
    let l2 list<int> = []
",
        vec![("l2", type_list(type_int()))],
    );
}

#[test]
fn test_nested_list() {
    check_expr_type("[[1, 2], [3, 4]]", type_list(type_list(type_int())));
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
fn test_list_slicing() {
    check_expr_type("[1, 2, 3][0..1]", type_list(type_int()));
    check_expr_type("[1, 2, 3][0..=1]", type_list(type_int()));
}

#[test]
fn test_list_slicing_variable() {
    check_expr_type(
        "
let r = 0..1
[1, 2, 3][r]
",
        type_list(type_int()),
    );
}
