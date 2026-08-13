// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

#![allow(clippy::approx_constant)]

use super::utils::{parser_error_test, parser_test, run_float_tests};
use miri::ast::factory::{
    binary, call, expression_statement, float64, float64_literal_expression, identifier,
    let_variable, member, unary, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility, UnaryOp};
use miri::error::syntax::SyntaxErrorKind;

/// A source float literal carries the full precision of what was written. The
/// parser assigns no width of its own — the type checker decides that from the
/// context the literal lands in — so every literal here is `f64`, including the
/// ones whose value would survive a round trip through `f32`.
#[test]
fn test_parse_float_literal() {
    run_float_tests(vec![
        ("3.14", float64(3.14)),
        ("1.797693134862315", float64(1.797693134862315)),
        ("1_000.0", float64(1_000.0)),
        ("1_000_000.123456789", float64(1_000_000.123456789)),
        ("1.0e10", float64(1.0e10)),
        ("6.67430e-11", float64(6.67430e-11)),
    ]);
}

#[test]
fn test_parse_float_literal_edge_cases() {
    run_float_tests(vec![
        // Precision: a literal that fits f32 is not rounded to it.
        ("3.141592", float64(3.141592)),
        ("3.1415927", float64(3.1415927)),
        ("3.14159265", float64(3.14159265)),
        // Largest and smallest values
        ("3.4028235e38", float64(3.4028235e38)), // max f32
        ("1.17549435e-38", float64(1.17549435e-38)), // min normal f32
        ("1.7976931348623157e308", float64(1.7976931348623157e308)), // max f64
        ("2.2250738585072014e-308", float64(2.2250738585072014e-308)), // min normal f64
        // Zeros
        ("0.0", float64(0.0)),
        ("0.000000", float64(0.0)),
        // Underscore formatting
        ("123_456.789", float64(123_456.789)),
        ("1_000_000.1234567", float64(1_000_000.1234567)),
        ("1_000_000.12345678", float64(1_000_000.12345678)),
        // Scientific notation variants
        ("1.0e+10", float64(1.0e+10)),
        ("1.0E10", float64(1.0E10)),
        ("1.0000001e10", float64(1.0000001e10)),
        ("9.999999e+37", float64(9.999999e37)),
        // Negative exponent
        ("1.0e-10", float64(1.0e-10)),
        ("6.02214076e-23", float64(6.02214076e-23)), // Planck constant
        // Values below what f32 can represent keep their value rather than
        // flushing to zero, which is the point of not pre-rounding.
        ("1e-46", float64(1e-46)),
        ("1e-39", float64(1e-39)),
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
                opt_expr(float64_literal_expression(3.14)),
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
            float64_literal_expression(3.14),
        ))],
    );
}

#[test]
fn test_float_in_binary_expression() {
    parser_test(
        "1.5 + 2.5",
        vec![expression_statement(binary(
            float64_literal_expression(1.5),
            BinaryOp::Add,
            float64_literal_expression(2.5),
        ))],
    );
}

#[test]
fn test_float_as_method_call_target() {
    parser_test(
        "3.14.round()",
        vec![expression_statement(call(
            member(float64_literal_expression(3.14), identifier("round")),
            vec![],
        ))],
    );
}

#[test]
fn test_float_overflow_parses_as_infinity() {
    // IEEE 754: overflow produces infinity, not an error.
    // This lets stdlib constants like `INF = 1e309` express true IEEE infinity.
    run_float_tests(vec![("1.8e309", float64(f64::INFINITY))]);
}

#[test]
fn test_error_float_followed_by_identifier() {
    // `3.14abc` tokenizes as a float then an identifier with no separator;
    // the statement must end after the float expression.
    parser_error_test(
        "3.14abc",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: "identifier".to_string(),
        },
    );
}
