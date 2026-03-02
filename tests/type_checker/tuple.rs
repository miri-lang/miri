// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_tuple_literal() {
    type_checker_test("(1, \"a\")");
}

#[test]
fn test_tuple_indexing() {
    type_checker_expr_type_test("(1, \"a\")[0]", type_int());
    type_checker_expr_type_test("(1, \"a\")[1]", type_string());
}

#[test]
fn test_tuple_indexing_out_of_bounds() {
    type_checker_error_test("(1, \"a\")[2]", "Tuple index out of bounds");
}

#[test]
fn test_tuple_indexing_variable_homogeneous() {
    // Should succeed because tuple is homogeneous (int, int)
    type_checker_expr_type_test(
        "
let i = 0
(1, 2)[i]
",
        type_int(),
    );
}

#[test]
fn test_tuple_indexing_variable_heterogeneous() {
    // Should fail because tuple is heterogeneous (int, string)
    type_checker_error_test(
        "
let i = 0
(1, \"a\")[i]
",
        "Tuple index must be an integer literal for heterogeneous tuples",
    );
}

#[test]
fn test_tuple_slicing() {
    // Slicing homogeneous tuple returns a list
    type_checker_expr_type_test("(1, 2, 3)[0..1]", type_list(type_int()));
}

#[test]
fn test_tuple_slicing_heterogeneous_error() {
    type_checker_error_test("(1, \"a\")[0..1]", "Cannot slice heterogeneous tuple");
}

#[test]
fn test_empty_tuple() {
    type_checker_test("()");
}

#[test]
fn test_empty_tuple_indexing() {
    type_checker_error_test("()[0]", "Tuple index out of bounds");
}

#[test]
fn test_nested_tuple() {
    type_checker_test("((1, 2), (3, 4))");
    type_checker_expr_type_test("((1, 2), (3, 4))[0][0]", type_int());
}

#[test]
fn test_nested_heterogeneous_tuple() {
    type_checker_test("((1, \"a\"), (2, \"b\"))");
    type_checker_expr_type_test("((1, \"a\"), (2, \"b\"))[0][1]", type_string());
}

#[test]
fn test_tuple_negative_index_heterogeneous() {
    type_checker_error_test("(1, \"a\")[-1]", "Tuple index must be an integer literal");
}

#[test]
fn test_tuple_match_success() {
    type_checker_test(
        "
match (1, \"a\")
    (i, s): i
",
    );
}

#[test]
fn test_tuple_match_type_inference() {
    type_checker_expr_type_test(
        "
match (1, \"a\")
    (i, s): i
",
        type_int(),
    );
}

#[test]
fn test_tuple_match_length_mismatch() {
    type_checker_error_test(
        "
match (1, \"a\")
    (i): i
",
        "Tuple pattern length mismatch",
    );
}

#[test]
fn test_tuple_match_type_mismatch() {
    type_checker_error_test(
        "
match (1, \"a\")
    (i, 1): i // 1 is int, but second element is String
",
        "Pattern type mismatch",
    );
}

#[test]
fn test_tuple_match_nested() {
    type_checker_expr_type_test(
        "
match ((1, 2), 3)
    ((a, b), c): b
",
        type_int(),
    );
}

#[test]
fn test_tuple_match_not_tuple() {
    type_checker_error_test(
        "
match 1
    (a, b): a
",
        "Expected tuple type for tuple pattern",
    );
}

#[test]
fn test_tuple_as_function_arg() {
    type_checker_test(
        "
fn f(t (int, String))
    return
f((1, \"a\"))
",
    );
}

#[test]
fn test_tuple_return_type() {
    type_checker_test(
        "
fn f() (int, String)
    return (1, \"a\")
",
    );
}

#[test]
fn test_tuple_return_type_mismatch() {
    type_checker_error_test(
        "
fn f() (int, String)
    return (1, 1)
",
        "Invalid return type",
    );
}
