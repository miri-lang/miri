// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::parser_test;
use miri::ast::factory::{binary, identifier, int_literal_expression, return_statement};
use miri::ast::{opt_expr, BinaryOp, ExpressionKind, IfStatementType, StatementKind};

#[test]
fn test_return_statement() {
    parser_test(
        "
return 42
",
        vec![return_statement(opt_expr(int_literal_expression(42)))],
    );
}

#[test]
fn test_return_statement_with_expression() {
    parser_test(
        "
return 42 + x
",
        vec![return_statement(opt_expr(binary(
            int_literal_expression(42),
            BinaryOp::Add,
            identifier("x"),
        )))],
    );
}

#[test]
fn test_empty_return_statement() {
    parser_test(
        "
return
",
        vec![return_statement(None)],
    );
}

#[test]
fn test_return_value_with_postfix_if_is_conditional_expression() {
    // `return 42 if x > 0` returns the conditional expression `42 if x > 0`;
    // the postfix guard binds to the value, not to the return statement.
    let result = super::utils::parse_program("return 42 if x > 0");
    assert_eq!(result.body.len(), 1);
    let StatementKind::Return(Some(value)) = &result.body[0].node else {
        panic!("Expected return with value, got {:?}", result.body[0].node);
    };
    let ExpressionKind::Conditional(then_value, condition, else_value, if_type) = &value.node
    else {
        panic!("Expected conditional expression, got {:?}", value.node);
    };
    assert_eq!(*if_type, IfStatementType::If);
    assert!(matches!(&then_value.node, ExpressionKind::Literal(_)));
    assert!(matches!(&condition.node, ExpressionKind::Binary(..)));
    assert!(else_value.is_none(), "Should not have an else branch");
}

#[test]
fn test_return_value_with_postfix_unless_is_conditional_expression() {
    // `return 42 unless x > 0` returns the conditional expression
    // `42 unless x > 0`; the guard binds to the value.
    let result = super::utils::parse_program("return 42 unless x > 0");
    assert_eq!(result.body.len(), 1);
    let StatementKind::Return(Some(value)) = &result.body[0].node else {
        panic!("Expected return with value, got {:?}", result.body[0].node);
    };
    let ExpressionKind::Conditional(then_value, condition, else_value, if_type) = &value.node
    else {
        panic!("Expected conditional expression, got {:?}", value.node);
    };
    assert_eq!(*if_type, IfStatementType::Unless);
    assert!(matches!(&then_value.node, ExpressionKind::Literal(_)));
    assert!(matches!(&condition.node, ExpressionKind::Binary(..)));
    assert!(else_value.is_none(), "Should not have an else branch");
}

#[test]
fn test_bare_return_with_postfix_if_guard_wraps_in_if_statement() {
    // A value-less `return if x > 0` is wrapped: `if x > 0: return`.
    let result = super::utils::parse_program("return if x > 0");
    assert_eq!(result.body.len(), 1);
    let StatementKind::If(condition, then_body, else_body, if_type) = &result.body[0].node else {
        panic!("Expected if statement, got {:?}", result.body[0].node);
    };
    assert_eq!(*if_type, IfStatementType::If);
    assert!(matches!(&condition.node, ExpressionKind::Binary(..)));
    assert!(
        matches!(&then_body.node, StatementKind::Return(None)),
        "Expected bare return in then body, got {:?}",
        then_body.node
    );
    assert!(else_body.is_none(), "Should not have else body");
}

#[test]
fn test_bare_return_with_postfix_unless_guard_wraps_in_unless_statement() {
    // A value-less `return unless done` is wrapped: `unless done: return`.
    let result = super::utils::parse_program("return unless done");
    assert_eq!(result.body.len(), 1);
    let StatementKind::If(condition, then_body, else_body, if_type) = &result.body[0].node else {
        panic!("Expected unless statement, got {:?}", result.body[0].node);
    };
    assert_eq!(*if_type, IfStatementType::Unless);
    assert!(matches!(&condition.node, ExpressionKind::Identifier(..)));
    assert!(
        matches!(&then_body.node, StatementKind::Return(None)),
        "Expected bare return in then body, got {:?}",
        then_body.node
    );
    assert!(else_body.is_none(), "Should not have else body");
}
