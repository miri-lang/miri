// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}};

use super::utils::*;


#[test]
fn test_function_with_no_params() {
    lexer_test("
// Function with no parameters
fn fancy_print
  print \"Hello, World!\"
", vec![
        Token::Fn, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::String, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_function_with_params() {
    lexer_test("
/* Function with parameters */
fn square(x int) int
  x * x

/* Another function example */
fn add(a int, b int) int
  a + b
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,

        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Plus, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_inline_function() {
    lexer_test("
// Inline function
fn multiply(a int, b int) int: a * b
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_lambda_function() {
    lexer_test("
// Lambda function
let f = (x int) int: x * x
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_multiline_lambda_function() {
    lexer_test("
// Multiline lambda function
let f1 = (a float, b float)
  print(a + b)
  print(a - b)
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Plus, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Minus, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_function_calls_with_parentheses() {
    lexer_test("
// Call with parentheses
fancy_print()
f(10)
f1(5.0, 3.0)
", vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::Int, Token::RParen, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::Float, Token::Comma, Token::Float, Token::RParen, Token::ExpressionStatementEnd,
    ]);
}


#[test]
fn test_function_call_with_codeblock() {
    lexer_test("
// Code block
let y = arr.map(
  fn (x int): x * 2
)
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Identifier, Token::Dot, Token::Identifier, Token::LParen,
            Token::Fn, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Colon, Token::Identifier, Token::Star, Token::Int,
        Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_namespaced_function_call() {
    lexer_test("
Http::new(url)
", vec![
        Token::Identifier, Token::DoubleColon, Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_lambda_with_empty_body() {
    lexer_test("
let f = fn()
    // empty body
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Fn, Token::LParen, Token::RParen, Token::ExpressionStatementEnd
    ]);
}
