// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::Token;

use super::utils::*;

#[test]
fn test_if_statement() {
    lexer_test(
        "
if x
    x = 10
else
    x = 20
    ",
        vec![
            Token::If,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_if_block_else_inline() {
    lexer_test(
        "
if x
    x = 10
else: x = 20
",
        vec![
            Token::If,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::Colon,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_if_inline_else_block() {
    lexer_test(
        "
if x: x = 10
else
    x = 20
",
        vec![
            Token::If,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_if_statement_with_assignment() {
    lexer_test(
        "
let y = if x > 10
    10
else
    20
    ",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_if_statement_inline() {
    lexer_test(
        "
if x: x = 10 else: x = 20
",
        vec![
            Token::If,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::Else,
            Token::Colon,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_if_statement_inline_with_assignment() {
    lexer_test(
        "
let x = 50
let y = if x % 2 == 0: x * x else: x / x
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::If,
            Token::Identifier,
            Token::Percent,
            Token::Int,
            Token::Equal,
            Token::Int,
            Token::Colon,
            Token::Identifier,
            Token::Star,
            Token::Identifier,
            Token::Else,
            Token::Colon,
            Token::Identifier,
            Token::Slash,
            Token::Identifier,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_if_with_empty_block() {
    lexer_test(
        "
if x
    // empty then
else
    x = 1
",
        vec![
            Token::If,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_if_with_empty_block_no_else() {
    lexer_test(
        "
if x
    // TODO
",
        vec![Token::If, Token::Identifier, Token::ExpressionStatementEnd],
    );
}

#[test]
fn test_if_with_comment_in_empty_block() {
    lexer_test(
        "
if x
    // This block is empty
let y = 1
",
        vec![
            Token::If,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_if_with_empty_else_block_with_followup() {
    lexer_test(
        "
if x
    x = 1
else
    // empty else
x = 2
",
        vec![
            Token::If,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}
