// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use miri::ast_factory::*;
use super::utils::*;


#[test]
fn test_continue_in_while_loop() {
    parser_test("
while x > 0
    if x == 1
        continue
    x -= 1
", vec![
        while_statement(
            binary(identifier("x"), BinaryOp::GreaterThan, int_literal_expression(0)),
            block(vec![
                if_statement(
                    binary(identifier("x"), BinaryOp::Equal, int_literal_expression(1)),
                    block(vec![continue_statement()]),
                    None
                ),
                expression_statement(
                    assign(
                        lhs_identifier("x"),
                        AssignmentOp::AssignSub,
                        int_literal_expression(1)
                    )
                )
            ])
        )
    ]);
}

#[test]
fn test_continue_in_nested_loop() {
    parser_test("
while a
    while b
        continue // continues inner loop only
", vec![
        while_statement(
            identifier("a"),
            block(vec![
                while_statement(
                    identifier("b"),
                    block(vec![continue_statement()])
                )
            ])
        )
    ]);
}

#[test]
fn test_error_continue_with_value() {
    parser_error_test(
        "while true: continue false",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: "false".to_string(),
        }
    );
}

#[test]
fn test_parse_continue_outside_loop() {
    parser_test("continue", vec![continue_statement()]);
}
