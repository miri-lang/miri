// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::BinaryOp;

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_string_literal() {
    parse_literal_test("'hello single quote'", string("hello single quote"));
    parse_literal_test("\"hello double quote\"", string("hello double quote"));
}

#[test]
fn test_f_string() {
    parse_test(
        r#"f"User: {name}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("User: "),
                    identifier("name"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_no_expressions() {
    // An f-string with no expressions should be parsed as a single literal part.
    parse_test(
        r#"f"just a regular string""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("just a regular string"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_starts_with_expression() {
    parse_test(
        r#"f"{name} is the user""#,
        vec![
            expression_statement(
                f_string(vec![
                    identifier("name"),
                    string_literal(" is the user"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_ends_with_expression() {
    parse_test(
        r#"f"The user is {name}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("The user is "),
                    identifier("name"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_adjacent_expressions() {
    parse_test(
        r#"f"{greeting}{separator}{name}""#,
        vec![
            expression_statement(
                f_string(vec![
                    identifier("greeting"),
                    identifier("separator"),
                    identifier("name"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_complex_expression() {
    parse_test(
        r#"f"Result: {10 * (x + y)}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Result: "),
                    binary(
                        int_literal_expression(10),
                        BinaryOp::Mul,
                        binary(
                            identifier("x"),
                            BinaryOp::Add,
                            identifier("y")
                        )
                    ),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_function_call() {
    parse_test(
        r#"f"Status: {get_status(200)}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Status: "),
                    call(
                        identifier("get_status"),
                        vec![int_literal_expression(200)]
                    ),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_single_quotes() {
    parse_test(
        r#"f'User: {name}'"#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("User: "),
                    identifier("name"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_nested_string_literal() {
    // Tests that the parser correctly handles a regular string inside an f-string expression.
    parse_test(
        r#"f"Greeting: {\"hello \" + name}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Greeting: "),
                    binary(
                        string_literal("hello "),
                        BinaryOp::Add,
                        identifier("name")
                    ),
                ])
            )
        ]
    );
}
