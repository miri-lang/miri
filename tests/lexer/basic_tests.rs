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
fn test_symbols_with_numbers() {
    lexer_test(":symbol123 :test_2 :_private", vec![
        Token::Symbol,
        Token::Symbol,
        Token::Symbol,
    ]);
}

#[test]
fn test_use() {
    lexer_test("
// Local module 
use Calc

// Global module
use System.Math

// Local module with path
use MyProject.Path.SomeModule

// Local module with path and alias
use Module2 as M2
", vec![
        Token::Use, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::As, Token::Identifier, Token::ExpressionStatementEnd,
    ]);
}


#[test]
fn test_declaration() {
    lexer_test("
let x = 10                                   // inferred
var y = 20                                   // mutable
let z int = 30                               // explicitly typed
let num = 5.0                                // float
let str string = 'Hello'                     // string
let is_active = true                         // boolean
let even = 10 % 2 == 0                       // even number check
let m = Map<string, int>()                   // map declaration
let arr1 = [10, 20, 30]                      // array
let arr2 [float] = [1.0, 2.0, 3.0]           // array with type
let dict1 = {key1: 'A', key2: 'B'}           // dictionary
let dict2 {string: int} = {key1: 1, key2: 2} // dictionary with type
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Var, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Float, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Identifier, Token::Assign, Token::String, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::True, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::Percent, Token::Int, Token::Equal, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Identifier, Token::LessThan, Token::Identifier, Token::Comma, Token::Identifier, Token::GreaterThan, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::LBracket, Token::Int, Token::Comma, Token::Int, Token::Comma, Token::Int, Token::RBracket, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::LBracket, Token::Identifier, Token::RBracket, Token::Assign, Token::LBracket, Token::Float, Token::Comma, Token::Float, Token::Comma, Token::Float, Token::RBracket, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::String, Token::Comma, Token::Identifier, Token::Colon, Token::String, Token::RBrace, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::LBrace, Token::Identifier, Token::Colon, Token::Identifier, Token::RBrace, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::Int, Token::Comma, Token::Identifier, Token::Colon, Token::Int, Token::RBrace, Token::ExpressionStatementEnd,
    ]);
}

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
fn test_invalid_characters() {
    lexer_error_test("valid @ invalid", SyntaxErrorKind::InvalidToken);
}

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

#[test]
fn test_large_nested_structure() {
    let mut input = String::new();
    let mut expected = Vec::new();
    
    for i in 0..100 {
        input.push_str(&format!("fn level{}()\n", i));
        input.push_str(&"    ".repeat(i + 1));
        expected.extend([Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd, Token::Indent]);
    }
    
    for _ in 0..100 {
        expected.push(Token::Dedent);
    }
    
    lexer_test(&input, expected);
}

#[test]
fn test_index_member_assignment() {
    lexer_test("
obj['prop'] = 1
", vec![
        Token::Identifier, Token::LBracket, Token::String, Token::RBracket, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
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
