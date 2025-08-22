// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_namespaced_function_call() {
    parse_test("Http::new(url)", vec![
        expression_statement(
            call(
                class_identifier("Http::new"),
                vec![identifier("url")]
            )
        )
    ]);
}

#[test]
fn test_namespaced_enum_access() {
    parse_test("let status = Http::Status.Ok", vec![
        variable_statement(vec![
            let_variable(
                "status",
                None,
                opt_expr(member(
                    class_identifier("Http::Status"),
                    identifier("Ok")
                ))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_namespaced_type_in_variable_declaration() {
    parse_variable_declaration_test(
        "let client Http::Client",
        vec![
            let_variable(
                "client",
                opt_expr(typ(Type::Custom("Http::Client".into(), None))),
                None
            )
        ],
        MemberVisibility::Public
    );
}

#[test]
fn test_namespaced_type_in_function_return() {
    parse_test("fn get_status() Http::Status: Http::Status.Ok", vec![
        func("get_status").return_type(
            typ(Type::Custom("Http::Status".into(), None)),
        ).build(
            expression_statement(
                member(
                    class_identifier("Http::Status"),
                    identifier("Ok")
                )
            )
        )
    ]);
}

#[test]
fn test_namespaced_type_in_function_parameter() {
    parse_test("fn set_status(s Http::Status): _status = s", vec![
        func("set_status").params(
            vec![
                parameter(
                    "s".into(),
                    opt_expr(typ(Type::Custom("Http::Status".into(), None))),
                    None
                )
            ]
        ).build(
            expression_statement(
                assign(
                    lhs_identifier("_status"),
                    AssignmentOp::Assign,
                    identifier("s")
                )
            )
        )
    ]);
}

#[test]
fn test_error_namespaced_variable_declaration() {
    // A variable name cannot be namespaced.
    parse_error_test(
        "let Http::x = 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "a simple identifier".to_string(),
            found: "Http::x".to_string(),
        }
    );
}

#[test]
fn test_error_namespaced_parameter_name() {
    // A function parameter name cannot be namespaced.
    parse_error_test(
        "fn my_func(Http::p int)",
        SyntaxErrorKind::UnexpectedToken {
            expected: "a simple identifier".to_string(),
            found: "Http::p".to_string(),
        }
    );
}

#[test]
fn test_error_namespaced_assignment_target() {
    // A namespaced identifier like `Http::Status` is a value, not a variable,
    // so it cannot be the direct target of an assignment.
    parse_error_test(
        "Http::Status = 'new_status'",
        SyntaxErrorKind::InvalidLeftHandSideExpression
    );
}
