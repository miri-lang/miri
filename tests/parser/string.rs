// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::opt_expr;
use miri::ast::BinaryOp;
use miri::ast::MemberVisibility;
use miri::error::syntax::SyntaxErrorKind;

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_parse_string_literal() {
    literal_test("'hello single quote'", string_literal("hello single quote"));
    literal_test(
        "\"hello double quote\"",
        string_literal("hello double quote"),
    );
}

#[test]
fn test_f_string() {
    parser_test(
        r#"f"User: {name}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("User: "),
            identifier("name"),
        ]))],
    );
}

#[test]
fn test_f_string_no_expressions() {
    // An f-string with no expressions should be parsed as a single literal part.
    parser_test(
        r#"f"just a regular string""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("just a regular string"),
        ]))],
    );
}

#[test]
fn test_f_string_starts_with_expression() {
    parser_test(
        r#"f"{name} is the user""#,
        vec![expression_statement(f_string(vec![
            identifier("name"),
            string_literal_expression(" is the user"),
        ]))],
    );
}

#[test]
fn test_f_string_ends_with_expression() {
    parser_test(
        r#"f"The user is {name}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("The user is "),
            identifier("name"),
        ]))],
    );
}

#[test]
fn test_f_string_with_adjacent_expressions() {
    parser_test(
        r#"f"{greeting}{separator}{name}""#,
        vec![expression_statement(f_string(vec![
            identifier("greeting"),
            identifier("separator"),
            identifier("name"),
        ]))],
    );
}

#[test]
fn test_f_string_with_complex_expression() {
    parser_test(
        r#"f"Result: {10 * (x + y)}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("Result: "),
            binary(
                int_literal_expression(10),
                BinaryOp::Mul,
                binary(identifier("x"), BinaryOp::Add, identifier("y")),
            ),
        ]))],
    );
}

#[test]
fn test_f_string_with_function_call() {
    parser_test(
        r#"f"Status: {get_status(200)}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("Status: "),
            call(identifier("get_status"), vec![int_literal_expression(200)]),
        ]))],
    );
}

#[test]
fn test_f_string_with_single_quotes() {
    parser_test(
        r#"f'User: {name}'"#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("User: "),
            identifier("name"),
        ]))],
    );
}

#[test]
fn test_f_string_with_nested_f_string() {
    parser_test(
        r#"f"Outer: {f'Inner: {x}'}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("Outer: "),
            f_string(vec![string_literal_expression("Inner: "), identifier("x")]),
        ]))],
    );
}

#[test]
fn test_f_string_with_map_literal() {
    parser_test(
        r#"f"Data: {{'key': value}}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("Data: "),
            map(vec![(
                string_literal_expression("key"),
                identifier("value"),
            )]),
        ]))],
    );
}

#[test]
fn test_f_string_with_escaped_braces_is_parsed_as_literal() {
    parser_test(
        r#"f"Literal braces \{ and \}""#,
        vec![expression_statement(f_string(vec![
            string_literal_expression("Literal braces \\{ and \\}"),
        ]))],
    );
}

#[test]
fn test_f_string_assigned_to_variable() {
    // Ensures f-strings work correctly as part of a larger statement.
    parser_test(
        r#"let message = f"Hello, {name}""#,
        vec![variable_statement(
            vec![let_variable(
                "message",
                None,
                opt_expr(f_string(vec![
                    string_literal_expression("Hello, "),
                    identifier("name"),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_method_call_on_string_literal() {
    parser_test(
        r#""hello".len()"#,
        vec![expression_statement(call(
            member(string_literal_expression("hello"), identifier("len")),
            vec![],
        ))],
    );
}

#[test]
fn test_method_call_on_f_string() {
    parser_test(
        r#"f"hello, {name}".upper()"#,
        vec![expression_statement(call(
            member(
                f_string(vec![
                    string_literal_expression("hello, "),
                    identifier("name"),
                ]),
                identifier("upper"),
            ),
            vec![],
        ))],
    );
}

#[test]
fn test_error_on_unclosed_f_string_expression() {
    parser_error_test(r#"f"Hello {name"#, &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_error_on_statement_in_f_string() {
    parser_error_test(
        r#"f"Value: {let x = 5}""#,
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "let".to_string(),
        },
    );
}

#[test]
fn test_error_on_backslash_in_f_string_expression() {
    parser_error_test(
        r#"f"Path: {\"C:\\Users\"}""#,
        &SyntaxErrorKind::BackslashInFStringExpression,
    );
}
