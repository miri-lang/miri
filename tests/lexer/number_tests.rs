// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::{Token};

use super::utils::*;


#[test]
fn test_number_edge_cases() {
    lexer_test("0 00 1_000_000 0.0 .5 5. -19 1.0e10 6.67430e-11 1E10 1e-5 1.5E+3 1.5e-10 1_000e10", vec![
        Token::Int,
        Token::Int,
        Token::Int,
        Token::Float,
        Token::Dot, Token::Int,  // .5 should be parsed as . and 5
        Token::Int, Token::Dot,  // 5. should be parsed as 5 and .,
        Token::Minus, Token::Int, // -19 should be parsed as Minus and 19
        // Scientific notation
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
    ]);
}

#[test]
fn test_float_precision_boundaries() {
    lexer_test("3.4028235e38 1.7976931348623157e308", vec![
        Token::Float, // f32 max
        Token::Float, // f64 max
    ]);
}

#[test]
fn test_integer_overflow_edge_cases() {
    lexer_test("9223372036854775807 9223372036854775808", vec![
        Token::Int, // i64::MAX
        Token::Int, // Should still tokenize, even if out of i64 range
    ]);
}

#[test]
fn test_very_large_numbers() {
    lexer_test("999999999999999999999999999999", vec![
        Token::Int, // Should tokenize even if unparseable
    ]);
}

#[test]
fn test_underscore_in_numbers() {
    lexer_test("1_2_3 4_5.6_7 1_234_567_890", vec![
        Token::Int,
        Token::Float,
        Token::Int,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers() {
    lexer_test("0b1010 0x1A2B 0x1fff 0o755", vec![
        Token::BinaryNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers_with_underscores() {
    lexer_test("0b1010_1010 0b1_0_1_0_1_0_1_0 0b_1111 0x1_A2_B 0xFaFa_EeEe 0x_abcd 0o7_5_5 0o755_7777 0o_777", vec![
        Token::BinaryNumber,
        Token::BinaryNumber,
        Token::BinaryNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::OctalNumber,
        Token::OctalNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers_incomplete() {
    lexer_test("0b 0x 0o", vec![
        Token::Int, // should not panic, just return other tokens
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
    ]);
}

// Note: this works, but maybe it shouldn't?
#[test]
fn test_binary_hex_octal_numbers_long_underscores() {
    lexer_test("0b___________ 0x___________ 0o___________", vec![
        Token::BinaryNumber, // should not panic, just return other tokens
        Token::HexNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_invalid_binary() {
    lexer_test("0b2 0bbb b111 0b1111_000F", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Identifier,
        Token::BinaryNumber,
        Token::Identifier,
    ]);
}

#[test]
fn test_invalid_hex() {
    lexer_test("0xPPPPp 0xxxx x00 0x0123z", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Identifier,
        Token::HexNumber,
        Token::Identifier,
    ]);
}

#[test]
fn test_invalid_octal() {
    lexer_test("0o8 0o9 0o7777z o7777", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::OctalNumber,
        Token::Identifier,
        Token::Identifier,
    ]);
}

#[test]
fn test_numbers_starting_with_dot() {
    // TODO: Should this be allowed? Works in Python, but not in Rust.
    lexer_test(".123", vec![Token::Dot, Token::Int]);
}

#[test]
fn test_numbers_ending_with_dot() {
    // TODO: Should this be allowed? Works in Python and Rust.
    lexer_test("123.", vec![Token::Int, Token::Dot]);
}

#[test]
fn test_hex_octal_binary_case_insensitivity() {
    lexer_test("0X1A 0B101 0O77", vec![
        Token::HexNumber,
        Token::BinaryNumber,
        Token::OctalNumber,
    ]);
}
