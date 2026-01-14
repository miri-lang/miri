// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::{lexer_error_test, lexer_token_test};

#[test]
fn test_very_long_identifier() {
    let mut long_id = String::from("v");
    long_id.push_str(&"ery".repeat(2000)); // 6000+ chars

    lexer_token_test(&long_id, vec![Token::Identifier]);
}

#[test]
fn test_many_tokens() {
    let mut code = String::new();
    let mut expected = Vec::new();

    // Create a long sequence of "x = 1;"
    for _ in 0..1000 {
        code.push_str("x = 1\n");
        expected.push(Token::Identifier);
        expected.push(Token::Assign);
        expected.push(Token::Int);
        expected.push(Token::ExpressionStatementEnd);
    }

    lexer_token_test(&code, expected);
}

#[test]
fn test_deeply_nested_grouping() {
    let depth = 500;
    let mut code = String::new();
    let mut expected = Vec::new();

    // (((...)))
    for _ in 0..depth {
        code.push('(');
        expected.push(Token::LParen);
    }
    for _ in 0..depth {
        code.push(')');
        expected.push(Token::RParen);
    }

    lexer_token_test(&code, expected);
}

#[test]
fn test_mixed_control_characters() {
    // Control characters other than standard whitespace should usually be errors or valid in strings.
    // In code:
    lexer_error_test("\x07", &SyntaxErrorKind::InvalidToken); // Bell
    lexer_error_test("\x1B", &SyntaxErrorKind::InvalidToken); // Escape
}

#[test]
fn test_null_byte() {
    // Null byte usually terminates strings in C, but in Rust it is a valid char.
    // However, in source code outside strings, it's invalid.
    lexer_error_test("\0", &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_null_byte_in_string() {
    // Valid in string
    lexer_token_test("\"\\0\"", vec![Token::String]);
    lexer_token_test("\"raw null \0 byte\"", vec![Token::String]);
}
