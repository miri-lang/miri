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
    run_lexer_tests(vec![
        ("123abc", vec![Token::Int, Token::Identifier]),
        ("abc123", vec![Token::Identifier]),
        ("123.456fn", vec![Token::Float, Token::Fn]),
    ]);
}

#[test]
fn test_unicode_identifiers() {
    run_lexer_error_tests(vec![
        "café",
        "naïve",
        "résumé",
    ], &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_symbol_identifier_boundaries() {
    run_lexer_tests(vec![
        (":symbol", vec![Token::Symbol]),
        ("identifier:", vec![Token::Identifier, Token::Colon]),
        (":one:two", vec![Token::Symbol, Token::Symbol]),
    ]);
}

#[test]
fn test_keywords_as_parts_of_identifiers() {
    run_lexer_tests(vec![
        ("if_condition", vec![Token::Identifier]),
        ("use_case", vec![Token::Identifier]),
        ("return_value", vec![Token::Identifier]),
    ]);
}

#[test]
fn test_case_sensitivity_of_keywords() {
    // Keywords are case-sensitive and must be lowercase.
    // Uppercase or mixed-case versions should be treated as identifiers.
    run_lexer_tests(vec![
        ("IF", vec![Token::Identifier]),
        ("TRUE", vec![Token::Identifier]),
        ("RETURN", vec![Token::Identifier]),
    ]);
}

#[test]
fn test_identifiers_with_underscores() {
    // Identifiers can start with, end with, or consist solely of underscores.
    run_lexer_tests(vec![
        ("_private", vec![Token::Identifier]),
        ("normal_", vec![Token::Identifier]),
        ("__special__", vec![Token::Identifier]),
        ("_", vec![Token::Identifier]),
    ]);
}

#[test]
fn test_identifier_operator_boundaries() {
    // The lexer should correctly separate identifiers from adjacent operators
    // without requiring whitespace.
    run_lexer_tests(vec![
        ("my_var+1", vec![Token::Identifier, Token::Plus, Token::Int]),
        ("counter++", vec![Token::Identifier, Token::Increment]),
        ("obj.property", vec![Token::Identifier, Token::Dot, Token::Identifier]),
        ("obj['prop'] = 1", vec![Token::Identifier, Token::LBracket, Token::String, Token::RBracket, Token::Assign, Token::Int]),
    ]);
}

#[test]
fn test_identifier_and_string_literal_boundaries() {
    run_lexer_tests(vec![
        (r#"my_func"string""#, vec![Token::Identifier, Token::String]),
        (r#"my_var'string'"#, vec![Token::Identifier, Token::String]),
    ]);
}

#[test]
fn test_identifier_in_range() {
    run_lexer_tests(vec![
        ("a..b", vec![Token::Identifier, Token::Range, Token::Identifier]),
        ("a..10", vec![Token::Identifier, Token::Range, Token::Int]),
        ("10..b", vec![Token::Int, Token::Range, Token::Identifier]),
        ("a..=b", vec![Token::Identifier, Token::RangeInclusive, Token::Identifier]),
        ("a..=10", vec![Token::Identifier, Token::RangeInclusive, Token::Int]),
        ("10..=b", vec![Token::Int, Token::RangeInclusive, Token::Identifier]),
    ]);
}
