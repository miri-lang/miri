// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

#![allow(clippy::approx_constant)]

use super::utils::{parser_error_test, parser_test, run_float_tests};
use miri::ast::factory::{
    binary, call, expression_statement, float32, float32_literal_expression, float64, identifier,
    let_variable, member, unary, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility, UnaryOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_float_literal() {
    run_float_tests(vec![
        ("3.14", float32(3.14)),
        ("1.797693134862315", float64(1.797693134862315)),
        ("1_000.0", float32(1_000.0)),
        ("1_000_000.123456789", float64(1_000_000.123456789)),
        ("1.0e10", float32(1.0e10)),
        ("6.67430e-11", float32(6.67430e-11)),
    ]);
}

#[test]
fn test_parse_float_literal_edge_cases() {
    run_float_tests(vec![
        // Precision edge cases
        ("3.141592", float32(3.141592)),     // fits f32
        ("3.1415927", float32(3.1415927)),   // still fits
        ("3.14159265", float64(3.14159265)), // too long for f32
        // Largest and smallest values
        ("3.4028235e38", float32(3.4028235e38)), // max f32
        ("1.17549435e-38", float32(1.175_494_4e-38)), // min normal f32
        ("1.7976931348623157e308", float64(1.7976931348623157e308)), // max f64
        ("2.2250738585072014e-308", float64(2.2250738585072014e-308)), // min normal f64
        // Zeros
        ("0.0", float32(0.0)),
        ("0.000000", float32(0.0)),
        // Underscore formatting
        ("123_456.789", float32(123_456.79)),
        ("1_000_000.1234567", float64(1_000_000.1234567)),
        ("1_000_000.12345678", float64(1_000_000.12345678)), // too long
        // Scientific notation variants
        ("1.0e+10", float32(1.0e+10)),
        ("1.0E10", float32(1.0E10)),
        ("1.0000001e10", float32(1.0000001e10_f32)), // precision edge
        ("9.999999e+37", float32(9.999999e37)),      // edge of f32
        // Negative exponent
        ("1.0e-10", float32(1.0e-10)),
        ("6.02214076e-23", float64(6.02214076e-23)), // Planck constant
        // Extreme edge underflow
        ("1e-46", float64(1e-46)), // below f32 subnormal
        ("1e-39", float32(1e-39)), // subnormal but fits
    ]);
}

#[test]
fn test_float_in_variable_declaration() {
    parser_test(
        "let x = 3.14",
        vec![variable_statement(
            vec![let_variable(
                "x",
                None,
                opt_expr(float32_literal_expression(3.14)),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_negative_float_expression() {
    parser_test(
        "-3.14",
        vec![expression_statement(unary(
            UnaryOp::Negate,
            float32_literal_expression(3.14),
        ))],
    );
}

#[test]
fn test_float_in_binary_expression() {
    parser_test(
        "1.5 + 2.5",
        vec![expression_statement(binary(
            float32_literal_expression(1.5),
            BinaryOp::Add,
            float32_literal_expression(2.5),
        ))],
    );
}

#[test]
fn test_float_as_method_call_target() {
    parser_test(
        "3.14.round()",
        vec![expression_statement(call(
            member(float32_literal_expression(3.14), identifier("round")),
            vec![],
        ))],
    );
}

#[test]
fn test_error_on_float_overflow() {
    parser_error_test("1.8e309", &SyntaxErrorKind::InvalidFloatLiteral);
}
