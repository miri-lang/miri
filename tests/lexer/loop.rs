// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}};

use super::utils::*;


#[test]
fn test_while_loop_nested_empty() {
    lexer_test("
while x > 0
    while y < 5
        // nested body
", vec![
        Token::While, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::While, Token::Identifier, Token::LessThan, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent
    ]);
}

#[test]
fn test_for_loop_nested_empty() {
    lexer_test("
for i in 1..3
    for c in \"ab\"
        // nested body
", vec![
        Token::For, Token::Identifier, Token::In, Token::Int, Token::Range, Token::Int, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::For, Token::Identifier, Token::In, Token::String, Token::ExpressionStatementEnd,
            Token::Dedent
    ]);
}