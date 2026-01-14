// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_use() {
    lexer_token_test(
        "
// System import
use system.math

// Local import
use local.users.user

// Selective import
use system.io.{print, println}

// Selective module import with rename
use system.{io, net as network}
",
        vec![
            // use system.math
            Token::Use,
            Token::System,
            Token::Dot,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            // use local.users.user
            Token::Use,
            Token::Local,
            Token::Dot,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            // use system.io.{print, println}
            Token::Use,
            Token::System,
            Token::Dot,
            Token::Identifier,
            Token::Dot,
            Token::LBrace,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::RBrace,
            Token::ExpressionStatementEnd,
            // use system.{io, net as network}
            Token::Use,
            Token::System,
            Token::Dot,
            Token::LBrace,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::As,
            Token::Identifier,
            Token::RBrace,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_use_with_keywords_in_path() {
    lexer_token_test(
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
