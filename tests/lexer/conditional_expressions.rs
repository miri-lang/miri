// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::Token;

use super::utils::*;

#[test]
fn test_conditional_expression() {
    lexer_test(
        "
let x = 10 if y > 5 else 20
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::Else,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_conditional_expression_no_else() {
    lexer_test(
        "
let x = 10 if y > 5
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_ternary_if_else_expression() {
    lexer_test(
        "let x = 10 if y > 5 else 20",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::Else,
            Token::Int,
        ],
    );
}

#[test]
fn test_chained_ternary_expressions() {
    lexer_test(
        "let x = 1 if a else 2 if b else 3",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::Else,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::Else,
            Token::Int,
        ],
    );
}

#[test]
fn test_ternary_as_return_value() {
    lexer_test(
        "return 1 if x else 0",
        vec![
            Token::Return,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::Else,
            Token::Int,
        ],
    );
}

#[test]
fn test_ternary_in_function_call() {
    lexer_test(
        "print(10 if x else 20)",
        vec![
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::Else,
            Token::Int,
            Token::RParen,
        ],
    );
}

#[test]
fn test_ternary_in_array_literal() {
    lexer_test(
        "let a = [1, 2, 10 if x else 20]",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::LBracket,
            Token::Int,
            Token::Comma,
            Token::Int,
            Token::Comma,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::Else,
            Token::Int,
            Token::RBracket,
        ],
    );
}

#[test]
fn test_multiline_ternary_expression() {
    lexer_test(
        "let x = 10 if y > 5\nelse 20",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Else,
            Token::Int,
        ],
    );
}
