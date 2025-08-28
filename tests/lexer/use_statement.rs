// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}};

use super::utils::*;


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