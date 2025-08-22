// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::{Token};

use super::utils::*;


#[test]
fn test_conditional_expression() {
    lexer_test("
let x = 10 if y > 5 else 20
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int,
        Token::If, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
        Token::Else, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_conditional_expression_no_else() {
    lexer_test("
let x = 10 if y > 5
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int,
        Token::If, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
    ]);
}
