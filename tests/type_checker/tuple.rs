// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_tuple_literal() {
    check_success("(1, \"a\")");
}

#[test]
fn test_tuple_indexing() {
    check_expr_type("(1, \"a\")[0]", type_int());
    check_expr_type("(1, \"a\")[1]", type_string());
}

#[test]
fn test_tuple_indexing_out_of_bounds() {
    check_error("(1, \"a\")[2]", "Tuple index out of bounds");
}

#[test]
fn test_tuple_indexing_variable_homogeneous() {
    // Should succeed because tuple is homogeneous (int, int)
    check_expr_type(
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
    check_error(
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
    check_expr_type("(1, 2, 3)[0..1]", type_list(type_int()));
}

#[test]
fn test_tuple_slicing_heterogeneous_error() {
    check_error("(1, \"a\")[0..1]", "Cannot slice heterogeneous tuple");
}

#[test]
fn test_empty_tuple() {
    check_success("()");
}

#[test]
fn test_empty_tuple_indexing() {
    check_error("()[0]", "Tuple index out of bounds");
}

#[test]
fn test_nested_tuple() {
    check_success("((1, 2), (3, 4))");
    check_expr_type("((1, 2), (3, 4))[0][0]", type_int());
}

#[test]
fn test_nested_heterogeneous_tuple() {
    check_success("((1, \"a\"), (2, \"b\"))");
    check_expr_type("((1, \"a\"), (2, \"b\"))[0][1]", type_string());
}

#[test]
fn test_tuple_negative_index_heterogeneous() {
    check_error("(1, \"a\")[-1]", "Tuple index must be an integer literal");
}

#[test]
fn test_tuple_match_success() {
    check_success(
        "
match (1, \"a\")
    (i, s): i
",
    );
}

#[test]
fn test_tuple_match_type_inference() {
    check_expr_type(
        "
match (1, \"a\")
    (i, s): i
",
        type_int(),
    );
}

#[test]
fn test_tuple_match_length_mismatch() {
    check_error(
        "
match (1, \"a\")
    (i): i
",
        "Tuple pattern length mismatch",
    );
}

#[test]
fn test_tuple_match_type_mismatch() {
    check_error(
        "
match (1, \"a\")
    (i, 1): i // 1 is int, but second element is string
",
        "Pattern type mismatch",
    );
}

#[test]
fn test_tuple_match_nested() {
    check_expr_type(
        "
match ((1, 2), 3)
    ((a, b), c): b
",
        type_int(),
    );
}

#[test]
fn test_tuple_match_not_tuple() {
    check_error(
        "
match 1
    (a, b): a
",
        "Expected tuple type for tuple pattern",
    );
}

#[test]
fn test_tuple_as_function_arg() {
    check_success(
        "
fn f(t (int, string))
    return
f((1, \"a\"))
",
    );
}

#[test]
fn test_tuple_return_type() {
    check_success(
        "
fn f() (int, string)
    return (1, \"a\")
",
    );
}

#[test]
fn test_tuple_return_type_mismatch() {
    check_error(
        "
fn f() (int, string)
    return (1, 1)
",
        "Invalid return type",
    );
}
