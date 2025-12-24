// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::type_list;
use miri::ast::Type;

#[test]
fn test_tuple_literal() {
    check_success("(1, \"a\")");
}

#[test]
fn test_tuple_indexing() {
    check_expr_type("(1, \"a\")[0]", Type::Int);
    check_expr_type("(1, \"a\")[1]", Type::String);
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
        Type::Int,
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
    check_expr_type("(1, 2, 3)[0..1]", type_list(Type::Int));
}

#[test]
fn test_tuple_slicing_heterogeneous_error() {
    check_error("(1, \"a\")[0..1]", "Cannot slice heterogeneous tuple");
}
