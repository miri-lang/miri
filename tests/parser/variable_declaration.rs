// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_variable_declaration() {
    variable_declaration_test(
        "let x",
        vec![
            let_variable(
                "x", 
                None, 
                None
            )
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_variable_declaration_with_initializer() {
    variable_declaration_test(
        "let x = 5",
        vec![
            let_variable(
                "x", 
                None, 
                opt_expr(int_literal_expression(5))
            )
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_typed_variable_declaration() {
    variable_declaration_test(
        "let x float",
        vec![
            let_variable(
                "x", 
                opt_expr(typ(Type::Float)), 
                None
            )
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_typed_variable_declaration_with_initializer() {
    variable_declaration_test(
        "let x int = 5",
        vec![
            let_variable(
                "x", 
                opt_expr(typ(Type::Int)), 
                opt_expr(int_literal_expression(5))
            )
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_invalid_variable_declaration() {
    // A literal cannot be a variable name.
    parser_error_test("let 123 = 456", &SyntaxErrorKind::UnexpectedToken {
        expected: "identifier".into(),
        found: "int".into(),
    });
}

#[test]
fn test_parse_mutable_variable_declaration() {
    variable_declaration_test(
        "var text = \"Hello, World!\"",
        vec![
            var("text", None, opt_expr(string_literal("Hello, World!")))
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_multiple_variable_declaration_no_initializer() {
    variable_declaration_test(
        "let x, y, z",
        vec![
            let_variable("x", None, None),
            let_variable("y", None, None),
            let_variable("z", None, None)
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_multiple_variable_declaration_mixed_initializer() {
    variable_declaration_test(
        "let x, y = 10, z",
        vec![
            let_variable("x", None, None),
            let_variable("y", None, opt_expr(int_literal_expression(10))),
            let_variable("z", None, None)
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_parse_variable_declaration_and_assignment() {
    parser_test("
var bar = 100
let foo = bar = 200
",
        vec![
            variable_statement(
                vec![var("bar", None, opt_expr(int_literal_expression(100)))],
                MemberVisibility::Public
            ),
            variable_statement(
                vec![let_variable("foo", None, opt_expr(assign(lhs_identifier("bar"), AssignmentOp::Assign, int_literal_expression(200))))],
                MemberVisibility::Public
            )
        ]
    );
}

#[test]
fn test_public_variable() {
    variable_declaration_test(
        "public let x = 1",
        vec![let_variable("x", None, opt_expr(int_literal_expression(1)))],
        MemberVisibility::Public
    );
}

#[test]
fn test_private_variable() {
    variable_declaration_test(
        "private var y",
        vec![var("y", None, None)],
        MemberVisibility::Private
    );
}

#[test]
fn test_error_modifier_order_variable() {
    // This is not a valid statement start.
    parser_error_test(
        "let public x = 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "public".to_string(),
        }
    );
}

#[test]
fn test_error_on_trailing_comma_in_declaration() {
    parser_error_test(
        "let x, y,",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_on_missing_variable_name() {
    parser_error_test(
        "let = 5",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "=".to_string(),
        }
    );

    parser_error_test(
        "let x, = 10",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "=".to_string(),
        }
    );
}
