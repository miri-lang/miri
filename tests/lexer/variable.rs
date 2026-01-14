// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_declaration() {
    lexer_token_test(
        "
let x = 10                                   // inferred
var y = 20                                   // mutable
let z int = 30                               // explicitly typed
let num = 5.0                                // float
let str string = 'Hello'                     // string
let is_active = true                         // boolean
let even = 10 % 2 == 0                       // even number check
let m = map<string, int>()                   // map declaration
let arr1 = [10, 20, 30]                      // array
let arr2 [float] = [1.0, 2.0, 3.0]           // array with type
let dict1 = {key1: 'A', key2: 'B'}           // dictionary
let dict2 {string: int} = {key1: 1, key2: 2} // dictionary with type
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Var,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Float,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Identifier,
            Token::Assign,
            Token::String,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::True,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::Percent,
            Token::Int,
            Token::Equal,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::LessThan,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::GreaterThan,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::LBracket,
            Token::Int,
            Token::Comma,
            Token::Int,
            Token::Comma,
            Token::Int,
            Token::RBracket,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::LBracket,
            Token::Identifier,
            Token::RBracket,
            Token::Assign,
            Token::LBracket,
            Token::Float,
            Token::Comma,
            Token::Float,
            Token::Comma,
            Token::Float,
            Token::RBracket,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::LBrace,
            Token::Identifier,
            Token::Colon,
            Token::String,
            Token::Comma,
            Token::Identifier,
            Token::Colon,
            Token::String,
            Token::RBrace,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::LBrace,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::RBrace,
            Token::Assign,
            Token::LBrace,
            Token::Identifier,
            Token::Colon,
            Token::Int,
            Token::Comma,
            Token::Identifier,
            Token::Colon,
            Token::Int,
            Token::RBrace,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_expression_end_after_collections() {
    lexer_token_test(
        "
let a = [1]
let b = 2
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::LBracket,
            Token::Int,
            Token::RBracket,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );

    lexer_token_test(
        "
let c = {k:1}
let d = 2
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::LBrace,
            Token::Identifier,
            Token::Colon,
            Token::Int,
            Token::RBrace,
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
fn test_keywords_as_variable_names() {
    // The lexer should tokenize these as keywords. The parser will later reject this.
    lexer_token_test(
        "
let let = 1
var if = 2
",
        vec![
            Token::Let,
            Token::Let,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Var,
            Token::If,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_declaration_without_whitespace() {
    lexer_token_test(
        "
let x=10
var y=20
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Var,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_multiple_declarations_on_one_line() {
    lexer_token_test(
        "let x, y = 1, 2",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::Comma,
            Token::Int,
        ],
    );
}
