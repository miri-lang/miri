// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test, type_statement_test};
use miri::ast::factory::{
    func, let_variable, parameter, type_expr_non_null, type_expr_option, type_int, type_rawptr,
    variable_statement,
};
use miri::ast::{opt_expr, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_rawptr_as_variable_type() {
    type_statement_test("RawPtr", type_expr_non_null(type_rawptr()));
}

#[test]
fn test_rawptr_nullable() {
    type_statement_test("RawPtr?", type_expr_option(type_rawptr()));
}

#[test]
fn test_rawptr_as_function_parameter() {
    parser_test(
        "\nfn alloc(size int) RawPtr\n    // body\n",
        vec![func("alloc")
            .params(vec![parameter(
                "size".into(),
                type_expr_non_null(type_int()),
                None,
                None,
            )])
            .return_type(type_expr_non_null(type_rawptr()))
            .build_empty_body()],
    );
}

#[test]
fn test_rawptr_as_function_return_type() {
    parser_test(
        "let ptr RawPtr",
        vec![variable_statement(
            vec![let_variable(
                "ptr",
                opt_expr(type_expr_non_null(type_rawptr())),
                None,
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_rawptr_double_nullable() {
    // `??` lexes as the null-coalescing operator, which then lacks an operand.
    parser_error_test(
        "let ptr RawPtr??",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "??".to_string(),
        },
    );
}

#[test]
fn test_error_rawptr_unterminated_parameter_list() {
    parser_error_test("fn alloc(size RawPtr", &SyntaxErrorKind::UnexpectedEOF);
}
