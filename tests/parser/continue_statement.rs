// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    assign, binary, block, continue_statement, expression_statement, identifier, if_statement,
    int_literal_expression, lhs_identifier, while_statement,
};
use miri::ast::{AssignmentOp, BinaryOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_continue_in_while_loop() {
    parser_test(
        "
while x > 0
    if x == 1
        continue
    x -= 1
",
        vec![while_statement(
            binary(
                identifier("x"),
                BinaryOp::GreaterThan,
                int_literal_expression(0),
            ),
            block(vec![
                if_statement(
                    binary(identifier("x"), BinaryOp::Equal, int_literal_expression(1)),
                    block(vec![continue_statement()]),
                    None,
                ),
                expression_statement(assign(
                    lhs_identifier("x"),
                    AssignmentOp::AssignSub,
                    int_literal_expression(1),
                )),
            ]),
        )],
    );
}

#[test]
fn test_continue_in_nested_loop() {
    parser_test(
        "
while a
    while b
        continue // continues inner loop only
",
        vec![while_statement(
            identifier("a"),
            block(vec![while_statement(
                identifier("b"),
                block(vec![continue_statement()]),
            )]),
        )],
    );
}

#[test]
fn test_error_continue_with_value() {
    parser_error_test(
        "while true: continue false",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: "false".to_string(),
        },
    );
}

#[test]
fn test_parse_continue_outside_loop() {
    parser_test("continue", vec![continue_statement()]);
}
