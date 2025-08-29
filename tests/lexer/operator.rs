// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::{Token};

use super::utils::*;


#[test]
fn test_symbols_and_operators() {
    lexer_test(": => -> <- || == != >= <= > < = + - * / % , . ( ) [ ] { } | & ^ .. ..= += -= *= /= %= ~ -- ++ ?", vec![
        Token::Colon,
        Token::FatArrow,
        Token::Arrow,
        Token::LeftArrow,
        Token::Parallel,
        Token::Equal,
        Token::NotEqual,
        Token::GreaterThanEqual,
        Token::LessThanEqual,
        Token::GreaterThan,
        Token::LessThan,
        Token::Assign,
        Token::Plus,
        Token::Minus,
        Token::Star,
        Token::Slash,
        Token::Percent,
        Token::Comma,
        Token::Dot,
        Token::LParen,
        Token::RParen,
        Token::LBracket,
        Token::RBracket,
        Token::LBrace,
        Token::RBrace,
        Token::Pipe,
        Token::Ampersand,
        Token::Caret,
        Token::Range,
        Token::RangeInclusive,
        Token::AssignAdd,
        Token::AssignSub,
        Token::AssignMul,
        Token::AssignDiv,
        Token::AssignMod,
        Token::Tilde,
        Token::Decrement,
        Token::Increment,
        Token::QuestionMark,
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
    // Should be parsed as -- and -
    lexer_test("---", vec![Token::Decrement, Token::Minus]);
    // Should be parsed as ++ and +
    lexer_test("+++", vec![Token::Increment, Token::Plus]);
    // Should be parsed as .. and .
    lexer_test("...", vec![Token::Range, Token::Dot]);
    // Should be parsed as two separate Pipe tokens, not one Parallel
    lexer_test("a | | b", vec![Token::Identifier, Token::Pipe, Token::Pipe, Token::Identifier]);
}
