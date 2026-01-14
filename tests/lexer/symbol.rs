// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::{lexer_token_test, run_lexer_tests};

#[test]
fn test_symbols_with_numbers() {
    lexer_token_test(
        ":symbol123 :test_2 :_private",
        vec![Token::Symbol, Token::Symbol, Token::Symbol],
    );
}

#[test]
fn test_symbol_vs_colon_operator() {
    run_lexer_tests(vec![
        (
            "a: b",
            vec![Token::Identifier, Token::Colon, Token::Identifier],
        ),
        ("a:b", vec![Token::Identifier, Token::Symbol]),
        ("a :b", vec![Token::Identifier, Token::Symbol]),
    ]);
}

#[test]
fn test_symbol_and_double_colon_boundary() {
    run_lexer_tests(vec![
        (
            "a::b",
            vec![Token::Identifier, Token::DoubleColon, Token::Identifier],
        ),
        (
            "a: :b",
            vec![Token::Identifier, Token::Colon, Token::Symbol],
        ),
    ]);
}

#[test]
fn test_invalid_symbol_formats() {
    run_lexer_tests(vec![
        (":123", vec![Token::Colon, Token::Int]),
        (":+", vec![Token::Colon, Token::Plus]),
        (":", vec![Token::Colon]),
    ]);
}

#[test]
fn test_symbol_with_keyword_name() {
    run_lexer_tests(vec![
        (":if", vec![Token::Symbol]),
        (":while", vec![Token::Symbol]),
        (":return", vec![Token::Symbol]),
    ]);
}

#[test]
fn test_symbol_and_operator_boundary() {
    run_lexer_tests(vec![
        (":symbol+1", vec![Token::Symbol, Token::Plus, Token::Int]),
        (
            "(:symbol)",
            vec![Token::LParen, Token::Symbol, Token::RParen],
        ),
    ]);
}
