// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;

#[test]
fn test_await_expression() {
    parser_test(
        "await some_future()",
        vec![expression_statement(unary(
            UnaryOp::Await,
            call(identifier("some_future"), vec![]),
        ))],
    );
}

#[test]
fn test_await_precedence_with_member_access() {
    // `await` has lower precedence than member access (`.`).
    // This should parse as `await (future.get_value())`.
    parser_test(
        "await future.get_value()",
        vec![expression_statement(unary(
            UnaryOp::Await,
            call(
                member(identifier("future"), identifier("get_value")),
                vec![],
            ),
        ))],
    );
}

// Note: `await` outside an `async` function is a semantic error, not a syntax error.
// The parser should successfully parse it.
#[test]
fn test_parse_await_in_non_async_function() {
    parser_test(
        "
fn not_async()
    await something()
",
        vec![
            func("not_async").build(block(vec![expression_statement(unary(
                UnaryOp::Await,
                call(identifier("something"), vec![]),
            ))])),
        ],
    );
}

#[test]
fn test_await_in_variable_assignment() {
    parser_test(
        "let result = await get_data()",
        vec![variable_statement(
            vec![let_variable(
                "result",
                None,
                opt_expr(unary(UnaryOp::Await, call(identifier("get_data"), vec![]))),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_chained_await_expression() {
    // `await` is right-associative, so this should parse as `await (await future)`.
    parser_test(
        "await await future",
        vec![expression_statement(unary(
            UnaryOp::Await,
            unary(UnaryOp::Await, identifier("future")),
        ))],
    );
}

#[test]
fn test_await_precedence_with_binary_expression() {
    // `await` has higher precedence than binary operators.
    // This should parse as `(await future) + 1`.
    parser_test(
        "await future + 1",
        vec![expression_statement(binary(
            unary(UnaryOp::Await, identifier("future")),
            BinaryOp::Add,
            int_literal_expression(1),
        ))],
    );
}

#[test]
fn test_await_in_function_argument() {
    parser_test(
        "my_func(await get_data())",
        vec![expression_statement(call(
            identifier("my_func"),
            vec![unary(UnaryOp::Await, call(identifier("get_data"), vec![]))],
        ))],
    );
}

#[test]
fn test_await_in_conditional_expression() {
    parser_test(
        "true if await is_ready() else false",
        vec![expression_statement(if_conditional(
            boolean_literal(true),
            unary(UnaryOp::Await, call(identifier("is_ready"), vec![])),
            Some(boolean_literal(false)),
        ))],
    );
}
