// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;



#[test]
fn test_very_long_identifier() {
    let long_name = "a".repeat(1000);
    lexer_test(&long_name, vec![Token::Identifier]);
}

#[test]
fn test_number_identifier_boundaries() {
    lexer_test("123abc abc123 123.456fn", vec![
        Token::Int, Token::Identifier,
        Token::Identifier,
        Token::Float, Token::Fn,
    ]);
}

#[test]
fn test_unicode_identifiers() {
    lexer_error_test("café naïve résumé", SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_symbol_identifier_boundaries() {
    lexer_test(":symbol identifier: :another", vec![
        Token::Symbol,
        Token::Identifier,
        Token::Colon,
        Token::Symbol,
    ]);
}

#[test]
fn test_keywords_as_parts_of_identifiers() {
    lexer_test("if_condition use_case return_value", vec![
        Token::Identifier,  // should not be parsed as "if"
        Token::Identifier,  // should not be parsed as "use"
        Token::Identifier,  // should not be parsed as "return"
    ]);
}
