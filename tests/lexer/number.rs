// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::*;

#[test]
fn test_valid_numbers() {
    run_lexer_tests(vec![
        // Normal numbers
        ("0", vec![Token::Int]),
        ("0.0", vec![Token::Float]),
        // Leading zeros
        ("00", vec![Token::Int]),
        ("000000.000000", vec![Token::Float]),
        // Underscores
        ("1_000_000", vec![Token::Int]),
        ("1_2_3", vec![Token::Int]),
        ("4_5.6_7", vec![Token::Float]),
        ("1_234_567_890", vec![Token::Int]),
        ("0b1010_1010", vec![Token::BinaryNumber]),
        ("0b1_0_1_0_1_0_1_0", vec![Token::BinaryNumber]),
        ("0b0000_1111", vec![Token::BinaryNumber]),
        ("0x1_A2_B", vec![Token::HexNumber]),
        ("0xFaFa_EeEe", vec![Token::HexNumber]),
        ("0xabcd_abcd", vec![Token::HexNumber]),
        ("0o7_5_5", vec![Token::OctalNumber]),
        ("0o755_7777", vec![Token::OctalNumber]),
        // Binary numbers
        ("0b1010", vec![Token::BinaryNumber]),
        ("0b1111", vec![Token::BinaryNumber]),
        ("0b0000_1111", vec![Token::BinaryNumber]),
        ("0B1010", vec![Token::BinaryNumber]),
        // Hexadecimal numbers
        ("0x1A", vec![Token::HexNumber]),
        ("0xFF", vec![Token::HexNumber]),
        ("0x0_1A", vec![Token::HexNumber]),
        ("0x0_1A_FF", vec![Token::HexNumber]),
        ("0XFFF", vec![Token::HexNumber]),
        // Octal numbers
        ("0o7", vec![Token::OctalNumber]),
        ("0o77", vec![Token::OctalNumber]),
        ("0o0_7", vec![Token::OctalNumber]),
        ("0o7_7_7", vec![Token::OctalNumber]),
        ("0O777", vec![Token::OctalNumber]),
        // Dot position
        (".5", vec![Token::Float]),
        ("5.", vec![Token::Float]),
        // Negative
        ("-19", vec![Token::Minus, Token::Int]),
        ("-19.0", vec![Token::Minus, Token::Float]),
        ("-.3", vec![Token::Minus, Token::Float]),
        ("-3.", vec![Token::Minus, Token::Float]),
        // Scientific notation
        ("1.0e10", vec![Token::Float]),
        ("6.67430e-11", vec![Token::Float]),
        ("1E10", vec![Token::Float]),     // Uppercase 'E'
        ("1e-5", vec![Token::Float]),     // Lowercase 'e' with negative exponent
        ("1.5E+3", vec![Token::Float]),   // Positive exponent with '+'
        ("1.5e-10", vec![Token::Float]),  // Lowercase 'e' with negative exponent
        ("1_000e10", vec![Token::Float]), // Underscore in integer part with scientific notation
    ]);
}

#[test]
fn test_float_precision_boundaries() {
    lexer_test(
        "3.4028235e38 1.7976931348623157e308",
        vec![
            Token::Float, // f32 max
            Token::Float, // f64 max
        ],
    );
}

#[test]
fn test_integer_overflow_edge_cases() {
    lexer_test(
        "9223372036854775807 9223372036854775808",
        vec![
            Token::Int, // i64::MAX
            Token::Int, // Should still tokenize, even if out of i64 range
        ],
    );
}

#[test]
fn test_very_large_numbers() {
    lexer_test(
        "999999999999999999999999999999",
        vec![
            Token::Int, // Should tokenize even if unparseable
        ],
    );
}

#[test]
fn test_invalid_underscore_in_numbers() {
    run_lexer_error_tests(
        vec!["1_2_", "_123", "1_2_3_"],
        &SyntaxErrorKind::InvalidNumberLiteral,
    );
}

#[test]
fn test_invalid_base_n_numbers_with_only_underscores() {
    run_lexer_error_tests(
        vec!["0b_", "0b___________"],
        &SyntaxErrorKind::InvalidBinaryLiteral,
    );

    run_lexer_error_tests(
        vec!["0x_", "0x___________"],
        &SyntaxErrorKind::InvalidHexLiteral,
    );

    run_lexer_error_tests(
        vec!["0o_", "0o___________"],
        &SyntaxErrorKind::InvalidOctalLiteral,
    );
}

#[test]
fn test_invalid_binary() {
    run_lexer_error_tests(
        vec!["0b", "0b2", "0bbb", "0b1111_000F", "0b+"],
        &SyntaxErrorKind::InvalidBinaryLiteral,
    );
}

#[test]
fn test_invalid_octal() {
    run_lexer_error_tests(
        vec!["0o", "0o8", "0o9", "0o7777z", "0o/"],
        &SyntaxErrorKind::InvalidOctalLiteral,
    );
}

#[test]
fn test_invalid_hex() {
    run_lexer_error_tests(
        vec!["0x", "0xPPPPp", "0xxxx", "0xG", "0x0123z", "0x1G", "0x-"],
        &SyntaxErrorKind::InvalidHexLiteral,
    );
}

#[test]
fn test_number_followed_by_dot_method_call() {
    // This is a critical test to ensure `1.to_string()` is not confused with a float.
    // The `FloatOrRange` logic should correctly see the `t` and treat `1.` as member call of an integer,
    lexer_test(
        "1.to_string()",
        vec![
            Token::Int,
            Token::Dot,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_number_in_range() {
    run_lexer_tests(vec![
        ("5..10", vec![Token::Int, Token::Range, Token::Int]),
        (
            "5..=10",
            vec![Token::Int, Token::RangeInclusive, Token::Int],
        ),
    ]);
}
