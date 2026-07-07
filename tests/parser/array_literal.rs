// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parse_program, parser_error_test, parser_test};
use miri::ast::factory::{array, expression_statement, int_literal_expression};
use miri::ast::{ExpressionKind, StatementKind};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_empty_array_literal() {
    let program = parse_program("[]");

    assert_eq!(program.body.len(), 1);
    match &program.body[0].node {
        StatementKind::Expression(expression) => match &expression.node {
            ExpressionKind::Array(elements, _) => assert!(elements.is_empty()),
            other => panic!("expected an array literal, found {:?}", other),
        },
        other => panic!("expected an expression statement, found {:?}", other),
    }
}

#[test]
fn test_array_literal_with_elements() {
    parser_test(
        "[10, 20]",
        vec![expression_statement(array(
            vec![int_literal_expression(10), int_literal_expression(20)],
            Box::new(int_literal_expression(2)),
        ))],
    );
}

#[test]
fn test_nested_array_literal() {
    let program = parse_program("[[1], [2]]");

    assert_eq!(program.body.len(), 1);
    match &program.body[0].node {
        StatementKind::Expression(expression) => match &expression.node {
            ExpressionKind::Array(elements, _) => {
                assert_eq!(elements.len(), 2);
                for element in elements {
                    assert!(matches!(element.node, ExpressionKind::Array(..)));
                }
            }
            other => panic!("expected an array literal, found {:?}", other),
        },
        other => panic!("expected an expression statement, found {:?}", other),
    }
}

#[test]
fn test_unclosed_array_literal_reports_error() {
    parser_error_test("let x = [1, 2", &SyntaxErrorKind::UnexpectedEOF);
}
