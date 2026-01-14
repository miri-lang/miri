// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_while_loop_nested_empty() {
    lexer_token_test(
        "
while x > 0
    while y < 5
        // nested body
",
        vec![
            Token::While,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::While,
            Token::Identifier,
            Token::LessThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_for_loop_nested_empty() {
    lexer_token_test(
        "
for i in 1..3
    for c in \"ab\"
        // nested body
",
        vec![
            Token::For,
            Token::Identifier,
            Token::In,
            Token::Int,
            Token::Range,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::For,
            Token::Identifier,
            Token::In,
            Token::String,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_until_loop() {
    lexer_token_test(
        "until x <= 0: x = x - 1",
        vec![
            Token::Until,
            Token::Identifier,
            Token::LessThanEqual,
            Token::Int,
            Token::Colon,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::Minus,
            Token::Int,
        ],
    );
}

#[test]
fn test_forever_loop() {
    lexer_token_test(
        "
forever
    if condition(): break
",
        vec![
            Token::Forever,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::If,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::Colon,
            Token::Break,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_do_while_loop() {
    lexer_token_test(
        "
do
    x = x + 1
while x < 10
",
        vec![
            Token::Do,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::While,
            Token::Identifier,
            Token::LessThan,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_for_loop_with_inclusive_range() {
    lexer_token_test(
        "for i in 1..=10",
        vec![
            Token::For,
            Token::Identifier,
            Token::In,
            Token::Int,
            Token::RangeInclusive,
            Token::Int,
        ],
    );
}

#[test]
fn test_for_loop_over_identifier() {
    lexer_token_test(
        "for item in my_list: print(item)",
        vec![
            Token::For,
            Token::Identifier,
            Token::In,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
        ],
    );
}

#[test]
fn test_loop_with_multiline_condition() {
    lexer_token_test(
        "
while (x > 0 and
       y < 0)
    x = x - 1
",
        vec![
            Token::While,
            Token::LParen,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::And,
            Token::Identifier,
            Token::LessThan,
            Token::Int,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::Minus,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}
