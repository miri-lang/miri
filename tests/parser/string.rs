// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::opt_expr;
use miri::ast::BinaryOp;
use miri::ast::MemberVisibility;

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_string_literal() {
    literal_test("'hello single quote'", string("hello single quote"));
    literal_test("\"hello double quote\"", string("hello double quote"));
}

#[test]
fn test_f_string() {
    parser_test(
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
    parser_test(
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
    parser_test(
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
    parser_test(
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
    parser_test(
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
    parser_test(
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
    parser_test(
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
    parser_test(
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
fn test_f_string_with_nested_f_string() {
    parser_test(
        r#"f"Outer: {f'Inner: {x}'}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Outer: "),
                    f_string(vec![
                        string_literal("Inner: "),
                        identifier("x"),
                    ]),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_map_literal() {
    parser_test(
        r#"f"Data: {{'key': value}}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Data: "),
                    map(vec![(
                        string_literal("key"),
                        identifier("value")
                    )]),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_with_escaped_braces_is_parsed_as_literal() {
    parser_test(
        r#"f"Literal braces \{ and \}""#,
        vec![
            expression_statement(
                f_string(vec![
                    string_literal("Literal braces \\{ and \\}"),
                ])
            )
        ]
    );
}

#[test]
fn test_f_string_assigned_to_variable() {
    // Ensures f-strings work correctly as part of a larger statement.
    parser_test(
        r#"let message = f"Hello, {name}""#,
        vec![
            variable_statement(
                vec![let_variable(
                    "message",
                    None,
                    opt_expr(f_string(vec![
                        string_literal("Hello, "),
                        identifier("name"),
                    ])),
                )],
                MemberVisibility::Public
            )
        ]
    );
}

