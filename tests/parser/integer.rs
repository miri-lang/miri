// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_integer_literal() {
    run_int_tests(vec![
        ("42", int(42)),
        ("12345", int(12345)),
        ("1_234_567_890", int(1234567890)),
        ("9_223_372_036_854_775_807", int(9223372036854775807)),

        ("0b1_01_010", int(42)),
        ("0xFF", int(255)),
        ("0o77", int(63)),
        ("0o1234567", int(342391))
    ]);
}

#[test]
fn test_integer_in_variable_declaration() {
    parser_test("let x = 10", vec![
        variable_statement(vec![
            let_variable(
                "x",
                None,
                opt_expr(int_literal_expression(10))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_negative_integer_expression() {
    parser_test("-42", vec![
        expression_statement(
            unary(
                UnaryOp::Negate,
                int_literal_expression(42)
            )
        )
    ]);
}

#[test]
fn test_integer_in_binary_expression() {
    parser_test("10 + 20", vec![
        expression_statement(
            binary(
                int_literal_expression(10),
                BinaryOp::Add,
                int_literal_expression(20)
            )
        )
    ]);
}

#[test]
fn test_integer_as_method_call_target() {
    parser_test("42.to_string()", vec![
        expression_statement(
            call(
                member(
                    int_literal_expression(42),
                    identifier("to_string")
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_error_on_integer_overflow() {
    // This value is larger than i128::MAX and should cause a parsing error.
    let overflow_val = "340282366920938463463374607431768211456"; // 2^128
    parser_error_test(overflow_val, &SyntaxErrorKind::InvalidIntegerLiteral);
}
