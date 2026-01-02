// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::Token;

use super::utils::*;

#[test]
fn test_all_keywords_in_various_contexts() {
    let keyword_map = vec![
        ("use", Token::Use),
        ("fn", Token::Fn),
        ("async", Token::Async),
        ("await", Token::Await),
        ("spawn", Token::Spawn),
        ("gpu", Token::Gpu),
        ("if", Token::If),
        ("unless", Token::Unless),
        ("else", Token::Else),
        ("match", Token::Match),
        ("default", Token::Default),
        ("return", Token::Return),
        ("while", Token::While),
        ("until", Token::Until),
        ("do", Token::Do),
        ("for", Token::For),
        ("forever", Token::Forever),
        ("in", Token::In),
        ("let", Token::Let),
        ("var", Token::Var),
        ("or", Token::Or),
        ("and", Token::And),
        ("not", Token::Not),
        ("true", Token::True),
        ("false", Token::False),
        ("from", Token::From),
        ("as", Token::As),
        ("break", Token::Break),
        ("continue", Token::Continue),
        ("is", Token::Is),
        ("extends", Token::Extends),
        ("includes", Token::Includes),
        ("implements", Token::Implements),
        ("type", Token::Type),
        ("enum", Token::Enum),
        ("struct", Token::Struct),
        ("public", Token::Public),
        ("protected", Token::Protected),
        ("private", Token::Private),
    ];

    for (keyword, token) in keyword_map {
        keyword_context_test(keyword, token);
    }
}

/// Tests a keyword in various contexts to ensure it's not confused with an identifier.
fn keyword_context_test(keyword: &str, expected_token: Token) {
    // A keyword should be a standalone token, but part of an identifier if it's not bounded.
    // Example for `if`: `if if_ok ifok ok_if "if" if-1`
    let test_string = format!("{kw} {kw}_ok {kw}ok ok_{kw} \"{kw}\" {kw}-1", kw = keyword);

    lexer_test(
        &test_string,
        vec![
            expected_token.clone(), // `if`
            Token::Identifier,      // `if_ok`
            Token::Identifier,      // `ifok`
            Token::Identifier,      // `ok_if`
            Token::String,          // `"if"`
            expected_token.clone(), // `if`
            Token::Minus,           // `-`
            Token::Int,             // `1`
        ],
    );
}

#[test]
fn test_keywords_are_case_sensitive() {
    // Keywords must be lowercase. Uppercase or mixed-case versions are identifiers.
    lexer_test(
        "IF TRUE RETURN",
        vec![Token::Identifier, Token::Identifier, Token::Identifier],
    );
    lexer_test(
        "If True Return",
        vec![Token::Identifier, Token::Identifier, Token::Identifier],
    );
}

#[test]
fn test_keyword_and_operator_boundary() {
    // The lexer should not require whitespace between a keyword and an operator.
    lexer_test(
        "if(true)",
        vec![Token::If, Token::LParen, Token::True, Token::RParen],
    );
    lexer_test("return-1", vec![Token::Return, Token::Minus, Token::Int]);
}

#[test]
fn test_in_not_keyword() {
    lexer_test("in not", vec![Token::In, Token::Not]);
}
