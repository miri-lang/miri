// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{literal_test, parser_error_test, parser_test, run_int_tests};
use miri::ast::factory::{
    binary, call, expression_statement, identifier, int, int_literal_expression, let_variable,
    member, unary, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility, UnaryOp};
use miri::error::syntax::SyntaxErrorKind;

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
        ("0o1234567", int(342391)),
    ]);
}

#[test]
fn test_integer_in_variable_declaration() {
    parser_test(
        "let x = 10",
        vec![variable_statement(
            vec![let_variable(
                "x",
                None,
                opt_expr(int_literal_expression(10)),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_negative_integer_expression() {
    parser_test(
        "-42",
        vec![expression_statement(unary(
            UnaryOp::Negate,
            int_literal_expression(42),
        ))],
    );
}

#[test]
fn test_integer_in_binary_expression() {
    parser_test(
        "10 + 20",
        vec![expression_statement(binary(
            int_literal_expression(10),
            BinaryOp::Add,
            int_literal_expression(20),
        ))],
    );
}

#[test]
fn test_integer_as_method_call_target() {
    parser_test(
        "42.to_string()",
        vec![expression_statement(call(
            member(int_literal_expression(42), identifier("to_string")),
            vec![],
        ))],
    );
}

#[test]
fn test_error_on_integer_overflow() {
    // This value is larger than i128::MAX and should cause a parsing error.
    let overflow_val = "340282366920938463463374607431768211456"; // 2^128
    parser_error_test(overflow_val, &SyntaxErrorKind::InvalidIntegerLiteral);
}

#[test]
fn test_integer_literal_bit_pattern_reinterpretation() {
    // Non-decimal (hex, binary, octal) literals in the range (i64::MAX..=u64::MAX]
    // are reinterpreted as two's-complement signed i64 values.
    run_int_tests(vec![
        ("0xFFFF_FFFF_FFFF_FFFF", int(-1)),
        ("0x8000_0000_0000_0000", int(i64::MIN as i128)),
        (
            "0b11111111_11111111_11111111_11111111_11111111_11111111_11111111_11111111",
            int(-1),
        ),
        ("0o1777777777777777777777", int(-1)),
        // Bit patterns beyond u64::MAX retain their full i128 value
        ("0x1_0000_0000_0000_0000", int(18446744073709551616)),
    ]);
}

#[test]
fn test_integer_literal_boundaries_and_underscores() {
    run_int_tests(vec![
        ("0", int(0)),
        ("0_0", int(0)),
        ("1_2_3", int(123)),
        ("0x0_F_F", int(255)),
        ("0b1_0_1", int(5)),
        ("0o7_7", int(63)),
        ("9_223_372_036_854_775_807", int(i64::MAX as i128)),
        ("170141183460469231731687303715884105727", int(i128::MAX)),
    ]);
}

#[test]
fn test_error_on_invalid_numeric_underscores() {
    // Leading/trailing underscores in decimal numbers
    parser_error_test("123_", &SyntaxErrorKind::InvalidNumberLiteral);
    parser_error_test("_123", &SyntaxErrorKind::InvalidNumberLiteral);

    // Leading underscore after prefix in non-decimal numbers
    parser_error_test("0x_FF", &SyntaxErrorKind::InvalidHexLiteral);
    parser_error_test("0b_101", &SyntaxErrorKind::InvalidBinaryLiteral);
    parser_error_test("0o_77", &SyntaxErrorKind::InvalidOctalLiteral);
}

#[test]
fn test_error_on_non_decimal_integer_overflow() {
    // Binary overflow (> i128::MAX)
    let bin_overflow = format!("0b1{}", "0".repeat(128)); // 2^128
    parser_error_test(&bin_overflow, &SyntaxErrorKind::InvalidBinaryLiteral);

    // Hex overflow (> i128::MAX)
    let hex_overflow = "0x100000000000000000000000000000000"; // 2^128
    parser_error_test(hex_overflow, &SyntaxErrorKind::InvalidHexLiteral);

    // Octal overflow (> i128::MAX)
    let oct_overflow = "0o4000000000000000000000000000000000000000000"; // 2^128
    parser_error_test(oct_overflow, &SyntaxErrorKind::InvalidOctalLiteral);
}

#[test]
fn test_parse_u128_integer_literal_above_i128_max() {
    // Values in range (i128::MAX, u128::MAX] parse into IntegerLiteral::U128.
    literal_test(
        "170141183460469231731687303715884105728",
        miri::ast::Literal::Integer(miri::ast::IntegerLiteral::U128(
            170141183460469231731687303715884105728,
        )),
    );
    literal_test(
        "340282366920938463463374607431768211455",
        miri::ast::Literal::Integer(miri::ast::IntegerLiteral::U128(u128::MAX)),
    );
}

#[test]
fn test_error_on_invalid_non_decimal_digits() {
    // Invalid digits in binary, octal, or hex literals
    parser_error_test("0b102", &SyntaxErrorKind::InvalidBinaryLiteral);
    parser_error_test("0o89", &SyntaxErrorKind::InvalidOctalLiteral);
    parser_error_test("0x12GH", &SyntaxErrorKind::InvalidHexLiteral);
}
