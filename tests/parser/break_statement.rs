// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::*;
use miri::ast_factory::*;
use miri::syntax_error::SyntaxErrorKind;

#[test]
fn test_break_in_for_loop() {
    parser_test(
        "
for i in 1..10
    if i == 5
        break
",
        vec![for_statement(
            vec![let_variable("i", None, None)],
            range(
                int_literal_expression(1),
                opt_expr(int_literal_expression(10)),
                RangeExpressionType::Exclusive,
            ),
            block(vec![if_statement(
                binary(identifier("i"), BinaryOp::Equal, int_literal_expression(5)),
                block(vec![break_statement()]),
                None,
            )]),
        )],
    );
}

#[test]
fn test_break_in_forever_loop() {
    parser_test(
        "
forever
    print('running')
    break
",
        vec![forever_statement(block(vec![
            expression_statement(call(
                identifier("print"),
                vec![string_literal_expression("running")],
            )),
            break_statement(),
        ]))],
    );
}

#[test]
fn test_break_in_nested_loop() {
    parser_test(
        "
for i in 1..3
    for j in 1..3
        if j == 2
            break // breaks inner loop only
",
        vec![for_statement(
            vec![let_variable("i", None, None)],
            range(
                int_literal_expression(1),
                opt_expr(int_literal_expression(3)),
                RangeExpressionType::Exclusive,
            ),
            block(vec![for_statement(
                vec![let_variable("j", None, None)],
                range(
                    int_literal_expression(1),
                    opt_expr(int_literal_expression(3)),
                    RangeExpressionType::Exclusive,
                ),
                block(vec![if_statement(
                    binary(identifier("j"), BinaryOp::Equal, int_literal_expression(2)),
                    block(vec![break_statement()]),
                    None,
                )]),
            )]),
        )],
    );
}

#[test]
fn test_error_break_with_value() {
    parser_error_test(
        "for x in y: break 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: "int".to_string(),
        },
    );
}

// Note: `break` or `continue` outside a loop is a *semantic* error, not a *syntactic* one.
// The parser should successfully parse it, and a later analysis pass would reject it.
#[test]
fn test_parse_break_outside_loop() {
    parser_test("break", vec![break_statement()]);
}
