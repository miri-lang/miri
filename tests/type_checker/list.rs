// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_list_literal_int() {
    check_expr_type(
        "[1, 2, 3]",
        Type::List(Box::new(miri::ast::IdNode::new(
            0,
            miri::ast::ExpressionKind::Type(Box::new(Type::Int), false),
            0..0,
        ))),
    );
}

#[test]
fn test_list_literal_string() {
    check_expr_type(
        "[\"a\", \"b\"]",
        Type::List(Box::new(miri::ast::IdNode::new(
            0,
            miri::ast::ExpressionKind::Type(Box::new(Type::String), false),
            0..0,
        ))),
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
