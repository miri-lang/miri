// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_map_literal() {
    check_success("{ \"a\": 1, \"b\": 2 }");
}

#[test]
fn test_map_literal_mixed_keys_error() {
    check_error("{ \"a\": 1, 2: 2 }", "Map keys must have the same type");
}

#[test]
fn test_map_literal_mixed_values_error() {
    check_error(
        "{ \"a\": 1, \"b\": \"s\" }",
        "Map values must have the same type",
    );
}

#[test]
fn test_map_indexing() {
    check_expr_type("{ \"a\": 1 }[\"a\"]", Type::Int);
}

#[test]
fn test_map_indexing_invalid_key_type() {
    check_error("{ \"a\": 1 }[1]", "Invalid map key type");
}

#[test]
fn test_map_indexing_variable() {
    check_expr_type(
        "
let k = \"a\"
{ \"a\": 1 }[k]
",
        Type::Int,
    );
}

#[test]
fn test_map_indexing_function_call() {
    check_expr_type(
        "
fn get_key() string
    return \"a\"

{ \"a\": 1 }[get_key()]
",
        Type::Int,
    );
}

#[test]
fn test_map_indexing_variable_type_mismatch() {
    check_error(
        "
let k = 1
{ \"a\": 1 }[k]
",
        "Invalid map key type",
    );
}
