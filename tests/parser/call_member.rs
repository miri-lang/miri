// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, call, expression_statement, identifier, index, int_literal_expression, member,
};
use miri::ast::{BinaryOp, ExpressionKind, StatementKind};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_member_access_simple() {
    parser_test(
        "obj.field",
        vec![expression_statement(member(
            identifier("obj"),
            identifier("field"),
        ))],
    );
}

#[test]
fn test_index_access_simple() {
    parser_test(
        "arr[0]",
        vec![expression_statement(index(
            identifier("arr"),
            int_literal_expression(0),
        ))],
    );
}

#[test]
fn test_call_access_no_args() {
    parser_test(
        "f()",
        vec![expression_statement(call(identifier("f"), vec![]))],
    );
}

#[test]
fn test_call_access_with_args() {
    parser_test(
        "f(1, 2)",
        vec![expression_statement(call(
            identifier("f"),
            vec![int_literal_expression(1), int_literal_expression(2)],
        ))],
    );
}

#[test]
fn test_chained_member_and_index() {
    // obj.field[0]
    parser_test(
        "obj.field[0]",
        vec![expression_statement(index(
            member(identifier("obj"), identifier("field")),
            int_literal_expression(0),
        ))],
    );
}

#[test]
fn test_chained_call_and_member() {
    // f().method()
    parser_test(
        "f().method()",
        vec![expression_statement(call(
            member(call(identifier("f"), vec![]), identifier("method")),
            vec![],
        ))],
    );
}

#[test]
fn test_tuple_field_access_single() {
    // `t.0` tokenizes as Identifier then Float(.0) and is rewritten into
    // member access with an integer property.
    parser_test(
        "t.0",
        vec![expression_statement(member(
            identifier("t"),
            int_literal_expression(0),
        ))],
    );
}

#[test]
fn test_tuple_field_access_nested() {
    // `t.0.1` nests two tuple-field rewrites.
    parser_test(
        "t.0.1",
        vec![expression_statement(member(
            member(identifier("t"), int_literal_expression(0)),
            int_literal_expression(1),
        ))],
    );
}

#[test]
fn test_cast_access_simple() {
    let result = super::utils::parse_program("x as int");
    assert_eq!(result.body.len(), 1);
    let StatementKind::Expression(expression) = &result.body[0].node else {
        panic!(
            "Expected expression statement, got {:?}",
            result.body[0].node
        );
    };
    let ExpressionKind::Cast(value, target_type) = &expression.node else {
        panic!("Expected cast expression, got {:?}", expression.node);
    };
    assert!(matches!(&value.node, ExpressionKind::Identifier(..)));
    assert!(
        matches!(&target_type.node, ExpressionKind::Type(..)),
        "Expected type expression as cast target, got {:?}",
        target_type.node
    );
}

#[test]
fn test_whitespace_comparison_not_generic() {
    // a < b (with whitespace) should parse as comparison, not generic
    parser_test(
        "a < b",
        vec![expression_statement(binary(
            identifier("a"),
            BinaryOp::LessThan,
            identifier("b"),
        ))],
    );
}

#[test]
fn test_member_access_error_dangling_dot() {
    parser_error_test(
        "obj.",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn test_index_access_error_unterminated() {
    parser_error_test("arr[", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_call_access_error_unterminated() {
    parser_error_test("f(", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_cast_access_error_dangling() {
    parser_error_test(
        "5 as",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "type".to_string(),
            found: "after 'as'".to_string(),
        },
    );
}

#[test]
fn test_call_member_dangling_less_than_reports_missing_expression() {
    // `a <` opens a comparison / generic-argument position that is never closed;
    // the block's `}` arrives where an operand is required, so the parser reports
    // the missing expression rather than panicking.
    parser_error_test(
        "fn foo() { a < }",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "}".to_string(),
        },
    );
}
