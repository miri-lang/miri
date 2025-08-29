// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::{Token};

use super::utils::*;


#[test]
fn test_keywords() {
    let keyword_map = vec![
        ("use", Token::Use),
        ("fn", Token::Fn),
        ("async", Token::Async),
        ("await", Token::Await),
        ("spawn", Token::Spawn),
        ("gpu", Token::Gpu),
        ("if", Token::If),
        ("unless", Token::Unless),
        ("match", Token::Match),
        ("default", Token::Default),
        ("return", Token::Return),
        ("while", Token::While),
        ("until", Token::Until),
        ("do", Token::Do),
        ("for", Token::For),
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
    ];

    for (keyword, token) in keyword_map {
        keyword_test(keyword, token);
    }
}

fn keyword_test(keyword: &str, expected: Token) {
    lexer_test(
        format!("{keyword} {keyword}() {keyword}.blah blah.{keyword} {keyword}blah blah{keyword} blah_{keyword} \"{keyword}\" /* {keyword} */ {keyword}1 {keyword}'a' {keyword}-1").as_str(),
        vec![
        expected.clone(),
        expected.clone(), Token::LParen, Token::RParen,
        expected.clone(), Token::Dot, Token::Identifier,
        Token::Identifier, Token::Dot, expected.clone(),
        Token::Identifier,
        Token::Identifier,
        Token::Identifier,
        Token::String,
        Token::Identifier,
        expected.clone(), Token::String,
        expected.clone(), Token::Minus, Token::Int,
    ]);
}

#[test]
fn test_in_not_keyword() {
    lexer_test(
        "in not",
        vec![
        Token::In,
        Token::Not,
    ]);
}

#[test]
fn test_else_keyword() {
    lexer_test(
        "else else() else.blah blah.else blah blahelse blah_else \"else\" /* else */",
        vec![
        Token::Else,
        Token::ExpressionStatementEnd, Token::Else, Token::LParen, Token::RParen,
        Token::ExpressionStatementEnd, Token::Else, Token::Dot, Token::Identifier,
        Token::Identifier, Token::Dot, Token::ExpressionStatementEnd, Token::Else,
        Token::Identifier,
        Token::Identifier,
        Token::Identifier,
        Token::String,
    ]);
}
