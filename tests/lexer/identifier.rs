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
    lexer_error_test("café naïve résumé", &SyntaxErrorKind::InvalidToken);
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
        Token::Identifier,
        Token::Identifier,
        Token::Identifier,
    ]);
}

#[test]
fn test_case_sensitivity_of_keywords() {
    // Keywords are case-sensitive and must be lowercase.
    // Uppercase or mixed-case versions should be treated as identifiers.
    lexer_test("IF TRUE RETURN", vec![
        Token::Identifier, Token::Identifier, Token::Identifier,
    ]);
    lexer_test("If True Return", vec![
        Token::Identifier, Token::Identifier, Token::Identifier,
    ]);
    // A correct keyword next to an identifier version.
    lexer_test("if If", vec![
        Token::If, Token::Identifier,
    ]);
}

#[test]
fn test_identifiers_with_underscores() {
    // Identifiers can start with, end with, or consist solely of underscores.
    lexer_test("_private", vec![Token::Identifier]);
    lexer_test("normal_", vec![Token::Identifier]);
    lexer_test("__special__", vec![Token::Identifier]);
    lexer_test("_", vec![Token::Identifier]);
}

#[test]
fn test_identifier_operator_boundaries() {
    // The lexer should correctly separate identifiers from adjacent operators
    // without requiring whitespace.
    lexer_test("my_var+1", vec![
        Token::Identifier, Token::Plus, Token::Int,
    ]);
    lexer_test("counter++", vec![
        Token::Identifier, Token::Increment,
    ]);
    lexer_test("obj.property", vec![
        Token::Identifier, Token::Dot, Token::Identifier,
    ]);
    lexer_test("obj['prop'] = 1", vec![
        Token::Identifier, Token::LBracket, Token::String, Token::RBracket, Token::Assign, Token::Int,
    ]);
}

#[test]
fn test_identifier_and_string_literal_boundaries() {
    // No whitespace between an identifier and a string literal.
    lexer_test(r#"my_func"string""#, vec![
        Token::Identifier, Token::String,
    ]);
    lexer_test(r#"my_var'string'"#, vec![
        Token::Identifier, Token::String,
    ]);
}
