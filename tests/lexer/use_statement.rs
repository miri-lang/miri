// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use std::vec;

use miri::lexer::Token;

use super::utils::*;

#[test]
fn test_use() {
    lexer_test(
        "
// Local module 
use Calc

// Global module
use System.Math

// Local module with path
use MyProject.Path.SomeModule

// Local module with path and alias
use Module2 as M2
",
        vec![
            Token::Use,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Use,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Use,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Use,
            Token::Identifier,
            Token::As,
            Token::Identifier,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_use_with_keywords_in_path() {
    lexer_test(
        "use My.if.Path as return",
        vec![
            Token::Use,
            Token::Identifier,
            Token::Dot,
            Token::If,
            Token::Dot,
            Token::Identifier,
            Token::As,
            Token::Return,
        ],
    );
}

#[test]
fn test_use_without_whitespace() {
    lexer_test(
        "use(MyModule)",
        vec![Token::Use, Token::LParen, Token::Identifier, Token::RParen],
    );
}
