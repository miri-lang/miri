// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::opt_expr;
use miri::ast::AssignmentOp;
use miri::ast::BinaryOp;
use miri::error::syntax::SyntaxErrorKind;

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_forever_loop() {
    parser_test(
        "
forever
    x
",
        vec![forever_statement(block(vec![expression_statement(
            identifier("x".into()),
        )]))],
    );
}

#[test]
fn test_forever_loop_with_comment() {
    parser_test(
        "
forever // This is an infinite loop
    x
",
        vec![forever_statement(block(vec![expression_statement(
            identifier("x".into()),
        )]))],
    );
}

#[test]
fn test_forever_loop_with_empty_body_and_comment() {
    parser_test(
        "
forever
    // This is an infinite loop
",
        vec![forever_statement(empty_statement())],
    );
}

#[test]
fn test_forever_loop_nested_with_empty_body_and_comment() {
    parser_test(
        "
forever
    forever
        // This is an infinite loop
",
        vec![forever_statement(block(vec![forever_statement(
            empty_statement(),
        )]))],
    );
}

#[test]
fn test_forever_loop_inline() {
    parser_test(
        "
forever: x
",
        vec![forever_statement(expression_statement(identifier(
            "x".into(),
        )))],
    );
}

#[test]
fn test_forever_loop_inline_with_empty_body_and_comment() {
    parser_test(
        "
forever: // This is an infinite loop
",
        vec![forever_statement(empty_statement())],
    );
}

#[test]
fn test_forever_loop_inline_nested_with_empty_body_and_comment() {
    parser_test(
        "
forever: forever: // This is an infinite loop
",
        vec![forever_statement(forever_statement(empty_statement()))],
    );
}

#[test]
fn test_forever_loop_with_break_and_continue() {
    parser_test(
        "
forever
    x += 1
    if x > 10: continue
    if x == 5: break
",
        vec![forever_statement(block(vec![
            expression_statement(assign(
                lhs_identifier("x"),
                AssignmentOp::AssignAdd,
                int_literal_expression(1),
            )),
            if_statement(
                binary(
                    identifier("x"),
                    BinaryOp::GreaterThan,
                    int_literal_expression(10),
                ),
                continue_statement(),
                None,
            ),
            if_statement(
                binary(identifier("x"), BinaryOp::Equal, int_literal_expression(5)),
                break_statement(),
                None,
            ),
        ]))],
    );
}

#[test]
fn test_forever_loop_with_return() {
    parser_test(
        "
forever
    if condition(): return 42
",
        vec![forever_statement(block(vec![if_statement(
            call(identifier("condition"), vec![]),
            return_statement(opt_expr(int_literal_expression(42))),
            None,
        )]))],
    );
}

#[test]
fn test_error_on_forever_without_body() {
    parser_error_test(
        "forever x",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a colon or an expression end".to_string(),
            found: "identifier".to_string(),
        },
    );
}
