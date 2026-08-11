// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_at_symbol_lexes_to_token() {
    lexer_token_test("@non_exhaustive", vec![Token::At, Token::Identifier]);
}

#[test]
fn test_at_symbol_with_string_argument() {
    lexer_token_test(
        "@ignore(\"reason\")",
        vec![
            Token::At,
            Token::Identifier,
            Token::LParen,
            Token::String,
            Token::RParen,
        ],
    );
}

#[test]
fn test_multiple_attributes_on_separate_lines() {
    lexer_token_test(
        "@non_exhaustive\n@must_use",
        vec![
            Token::At,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::At,
            Token::MustUse,
        ],
    );
}
