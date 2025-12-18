// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_formatted_string() {
    check_exprs_type(vec![
        ("f\"hello {1}\"", Type::String),
        ("f\"val: {true}\"", Type::String),
        ("f\"{1} + {2} = {3}\"", Type::String),
    ]);
}

#[test]
fn test_formatted_string_with_variables() {
    check_vars_type(
        "
let name = \"World\"
let greeting = f\"Hello {name}\"
",
        vec![("greeting", Type::String)],
    );
}

#[test]
fn test_formatted_string_nested_expressions() {
    check_exprs_type(vec![("f\"result: {1 + 2}\"", Type::String)]);
}

#[test]
fn test_formatted_string_undefined_variable() {
    check_error(
        "f\"Hello {undefined_var}\"",
        "Undefined variable: undefined_var",
    );
}

#[test]
fn test_formatted_string_type_error_in_expression() {
    check_error(
        "f\"result: {1 + 'x'}\"",
        "Invalid types for arithmetic operation",
    );
}
