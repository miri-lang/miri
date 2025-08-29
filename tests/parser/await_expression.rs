// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_await_expression() {
    parse_test("await some_future()", vec![
        expression_statement(
            unary(
                UnaryOp::Await,
                call(identifier("some_future"), vec![])
            )
        )
    ]);
}

#[test]
fn test_await_precedence_with_member_access() {
    // `await` has lower precedence than member access (`.`).
    // This should parse as `await (future.get_value())`.
    parse_test("await future.get_value()", vec![
        expression_statement(
            unary(
                UnaryOp::Await,
                call(
                    member(identifier("future"), identifier("get_value")),
                    vec![]
                )
            )
        )
    ]);
}

// Note: `await` outside an `async` function is a semantic error, not a syntax error.
// The parser should successfully parse it.
#[test]
fn test_parse_await_in_non_async_function() {
    parse_test("
fn not_async()
    await something()
", vec![
        func("not_async").build(
            block(vec![
                expression_statement(
                    unary(
                        UnaryOp::Await,
                        call(identifier("something"), vec![])
                    )
                )
            ])
        )
    ]);
}

#[test]
fn test_await_in_variable_assignment() {
    parse_test("let result = await get_data()", vec![
        variable_statement(vec![
            let_variable(
                "result",
                None,
                opt_expr(
                    unary(
                        UnaryOp::Await,
                        call(identifier("get_data"), vec![])
                    )
                )
            )
        ], MemberVisibility::Public)
    ]);
}
