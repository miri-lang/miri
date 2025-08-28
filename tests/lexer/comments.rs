// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;


#[test]
fn test_inline_comments() {
    lexer_test(r#"
var x = 10 // simple inline comment

print('Hello') // 👋 this is a friendly comment

use System.Math // use System.Math // with another comment inside

x = x + 1 // math: x becomes x + 1
"#, vec![
        Token::Var, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::String, Token::RParen, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Identifier, Token::Assign, Token::Identifier, Token::Plus, Token::Int, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_multiline_comments() {
    lexer_test(r#"
/**/

/* This is a single-line comment */

/*****************************************/

/* This is a basic
multiline comment
spanning three lines */
let some = "code"

/* Multiline comment with code inside:
var a = 5
print('ignored!')
*/

fn func() int: 10 + 10

/***
/* 
  /* nested */ 
*/ 
***/

/*

  |\_/|
  ( o.o )   <- Cat!
  > ^ <

This is a comment with ASCII art.

Symbols: /* nested? */ < > & ^ ~
*/

print("Hello") /* inline comment */
"#, vec![
        Token::Let, Token::Identifier, Token::Assign, Token::String, Token::ExpressionStatementEnd,
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::Identifier, Token::Colon,
            Token::Int, Token::Plus, Token::Int, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::String, Token::RParen, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_deeply_nested_comments() {
    lexer_test("before /* outer /* inner /* deepest */ inner */ outer */ after", vec![
        Token::Identifier, Token::Identifier
    ]);
}

#[test]
fn test_unclosed_nested_comment() {
    lexer_error_test("/* outer /* inner */ still open", &SyntaxErrorKind::UnclosedMultilineComment);
}

#[test]
fn test_comment_with_code_like_content() {
    lexer_test("/* func(): if else */ real_code", vec![
        Token::Identifier
    ]);
}

#[test]
fn test_comment_at_eof() {
    lexer_test("code // comment with no newline", vec![
        Token::Identifier,
    ]);
}

#[test]
fn test_nested_comments_with_strings() {
    lexer_test(r#"/* outer /* "string inside comment" */ outer */ code"#, vec![
        Token::Identifier,
    ]);
}
