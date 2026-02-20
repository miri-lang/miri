// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, variable_declaration_test};
use miri::ast::common::MemberVisibility;
use miri::ast::factory as ast;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn const_integer_literal() {
    variable_declaration_test(
        "const x = 5",
        vec![ast::const_variable(
            "x",
            None,
            Some(Box::new(ast::int_literal_expression(5))),
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn const_typed_integer() {
    variable_declaration_test(
        "const x i32 = 5",
        vec![ast::const_variable(
            "x",
            Some(Box::new(ast::type_expr_non_null(ast::type_i32()))),
            Some(Box::new(ast::int_literal_expression(5))),
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn const_string_literal() {
    variable_declaration_test(
        "const name = \"hello\"",
        vec![ast::const_variable(
            "name",
            None,
            Some(Box::new(ast::string_literal_expression("hello"))),
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn const_boolean_literal() {
    variable_declaration_test(
        "const flag = true",
        vec![ast::const_variable(
            "flag",
            None,
            Some(Box::new(ast::boolean_literal(true))),
        )],
        MemberVisibility::Public,
    );
}

#[test]
fn const_without_initializer_is_error() {
    parser_error_test(
        "const x",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "'=' (constant declaration must have an initializer)".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn const_typed_without_initializer_is_error() {
    parser_error_test(
        "const x i32",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "'=' (constant declaration must have an initializer)".to_string(),
            found: "end of file".to_string(),
        },
    );
}
