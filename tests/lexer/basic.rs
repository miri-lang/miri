// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::{lexer_error_test, lexer_token_test};

#[test]
fn test_empty_input() {
    lexer_token_test("", vec![]);
}

#[test]
fn test_whitespace_only() {
    lexer_token_test("   \t  \n  \r\n  ", vec![]);
}

#[test]
fn test_invalid_characters() {
    lexer_error_test("valid @ invalid", &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_shebang() {
    lexer_token_test(
        "#!/usr/bin/env miri\nlet x = 1",
        vec![Token::Let, Token::Identifier, Token::Assign, Token::Int],
    );
}

#[test]
fn test_input_with_bom() {
    lexer_token_test(
        "\u{FEFF}let x = 1",
        vec![Token::Let, Token::Identifier, Token::Assign, Token::Int],
    );
}
