// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::lexer_token_test;
use miri::lexer::Token;

#[test]
fn const_integer() {
    lexer_token_test(
        "const x = 10",
        vec![Token::Const, Token::Identifier, Token::Assign, Token::Int],
    );
}

#[test]
fn const_typed_integer() {
    lexer_token_test(
        "const x i32 = 10",
        vec![
            Token::Const,
            Token::Identifier,
            Token::Identifier,
            Token::Assign,
            Token::Int,
        ],
    );
}

#[test]
fn const_string() {
    lexer_token_test(
        "const name = \"hello\"",
        vec![
            Token::Const,
            Token::Identifier,
            Token::Assign,
            Token::String,
        ],
    );
}

#[test]
fn const_boolean() {
    lexer_token_test(
        "const flag = true",
        vec![Token::Const, Token::Identifier, Token::Assign, Token::True],
    );
}

#[test]
fn const_is_keyword_not_identifier() {
    lexer_token_test("const", vec![Token::Const]);
}
