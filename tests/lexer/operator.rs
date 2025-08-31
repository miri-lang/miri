// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::Token, syntax_error::SyntaxErrorKind};

use super::utils::*;


#[test]
fn test_symbols_and_operators() {
    run_lexer_tests(vec![
        (":", vec![Token::Colon]),
        ("=>", vec![Token::FatArrow]),
        ("->", vec![Token::Arrow]),
        ("<-", vec![Token::LeftArrow]),
        ("==", vec![Token::Equal]),
        ("!=", vec![Token::NotEqual]),
        (">=", vec![Token::GreaterThanEqual]),
        ("<=", vec![Token::LessThanEqual]),
        (">", vec![Token::GreaterThan]),
        ("<", vec![Token::LessThan]),
        ("=", vec![Token::Assign]),
        ("+", vec![Token::Plus]),
        ("-", vec![Token::Minus]),
        ("*", vec![Token::Star]),
        ("/", vec![Token::Slash]),
        ("%", vec![Token::Percent]),
        (",", vec![Token::Comma]),
        (".", vec![Token::Dot]),
        ("(", vec![Token::LParen]),
        (")", vec![Token::RParen]),
        ("[", vec![Token::LBracket]),
        ("]", vec![Token::RBracket]),
        ("{", vec![Token::LBrace]),
        ("}", vec![Token::RBrace]),
        ("|", vec![Token::Pipe]),
        ("&", vec![Token::Ampersand]),
        ("^", vec![Token::Caret]),
        ("..", vec![Token::Range]),
        ("..=", vec![Token::RangeInclusive]),
        ("+=", vec![Token::AssignAdd]),
        ("-=", vec![Token::AssignSub]),
        ("*=", vec![Token::AssignMul]),
        ("/=", vec![Token::AssignDiv]),
        ("%=", vec![Token::AssignMod]),
        ("~", vec![Token::Tilde]),
        ("--", vec![Token::Decrement]),
        ("++", vec![Token::Increment]),
        ("?", vec![Token::QuestionMark]),
    ]);
}

#[test]
fn test_operators_without_spaces() {
    lexer_test("a+=b*=c/=d%=e==f!=g>=h<=i", vec![
        Token::Identifier, Token::AssignAdd,
        Token::Identifier, Token::AssignMul,
        Token::Identifier, Token::AssignDiv,
        Token::Identifier, Token::AssignMod,
        Token::Identifier, Token::Equal,
        Token::Identifier, Token::NotEqual,
        Token::Identifier, Token::GreaterThanEqual,
        Token::Identifier, Token::LessThanEqual,
        Token::Identifier,
    ]);
}

#[test]
fn test_complex_assignment_chains() {
    lexer_test("a = b = c += d *= e", vec![
        Token::Identifier, Token::Assign,
        Token::Identifier, Token::Assign,
        Token::Identifier, Token::AssignAdd,
        Token::Identifier, Token::AssignMul,
        Token::Identifier,
    ]);
}

#[test]
fn test_ambiguous_operator_sequences() {
    run_lexer_tests(vec![
        ("a===b", vec![Token::Identifier, Token::Equal, Token::Assign, Token::Identifier]),
        ("a!==b", vec![Token::Identifier, Token::NotEqual, Token::Assign, Token::Identifier]),
        ("a<<=b", vec![Token::Identifier, Token::LessThan, Token::LessThanEqual, Token::Identifier]),
        ("a>>=b", vec![Token::Identifier, Token::GreaterThan, Token::GreaterThanEqual, Token::Identifier]),
        ("---", vec![Token::Decrement, Token::Minus]),
        ("+++", vec![Token::Increment, Token::Plus]),
        ("...", vec![Token::Range, Token::Dot]),
        ("a | | b", vec![Token::Identifier, Token::Pipe, Token::Pipe, Token::Identifier]),
        ("a|||b", vec![Token::Identifier, Token::Pipe, Token::Pipe, Token::Pipe, Token::Identifier]),
        ("a&&&b", vec![Token::Identifier, Token::Ampersand, Token::Ampersand, Token::Ampersand, Token::Identifier]),
    ]);
}

#[test]
fn test_double_colon_operator() {
    run_lexer_tests(vec![
        ("MyModule::MyClass", vec![Token::Identifier, Token::DoubleColon, Token::Identifier]),
        ("a:::b", vec![Token::Identifier, Token::DoubleColon, Token::Symbol]),
    ]);
}

#[test]
fn test_slash_operator_and_comment_boundaries() {
    run_lexer_tests(vec![
        ("a/b", vec![Token::Identifier, Token::Slash, Token::Identifier]),
        ("a/=b", vec![Token::Identifier, Token::AssignDiv, Token::Identifier]),
        ("a/ b", vec![Token::Identifier, Token::Slash, Token::Identifier]),
        ("a//b", vec![Token::Identifier]),
    ]);

    lexer_error_test("a/*b", &SyntaxErrorKind::UnclosedMultilineComment);
}

#[test]
fn test_operator_and_literal_boundaries() {
    run_lexer_tests(vec![
        ("1+2-3*4/5", vec![
            Token::Int, Token::Plus, Token::Int, Token::Minus, Token::Int,
            Token::Star, Token::Int, Token::Slash, Token::Int,
        ]),
        (r#""a"+"b""#, vec![
            Token::String, Token::Plus, Token::String,
        ]),
    ]);
}
