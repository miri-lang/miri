// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;


#[test]
fn test_empty_input() {
    lexer_test("", vec![]);
}

#[test]
fn test_whitespace_only() {
    lexer_test("   \t  \n  \r\n  ", vec![]);
}


#[test]
fn test_invalid_characters() {
    lexer_error_test("valid @ invalid", &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_shebang() {
    lexer_test("#!/usr/bin/env miri\nlet x = 1", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int
    ]);
}

#[test]
fn test_input_with_bom() {
    lexer_test("\u{FEFF}let x = 1", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int
    ]);
}

#[test]
fn test_index_member_assignment() {
    lexer_test("
obj['prop'] = 1
", vec![
        Token::Identifier, Token::LBracket, Token::String, Token::RBracket, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}
