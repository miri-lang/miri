// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::{lexer_error_test, lexer_token_test};

// ---------------------------------------------------------------------------
// EOF without trailing newline
// ---------------------------------------------------------------------------

#[test]
fn test_eof_no_newline_single_indent() {
    // Single indented statement, file ends right after identifier
    lexer_token_test(
        "if true\n    x",
        vec![
            Token::If,
            Token::True,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_eof_no_newline_double_dedent() {
    // Two levels of indentation, file ends without newline — both dedents emitted
    lexer_token_test(
        "fn f()\n    if true\n        x",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::If,
            Token::True,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_eof_no_newline_after_rparen() {
    // Indented function call ending with ')' and no newline
    lexer_token_test(
        "fn f()\n    g(1)",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_eof_no_newline_after_string() {
    // Indented string literal at EOF
    lexer_token_test(
        "fn f()\n    \"hello\"",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::String,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_eof_no_newline_flat_code() {
    // Flat code (no indentation) ending without newline — no ESE needed
    lexer_token_test(
        "let x = 1",
        vec![Token::Let, Token::Identifier, Token::Assign, Token::Int],
    );
}

#[test]
fn test_eof_no_newline_fstring_in_indented_block() {
    // Formatted string inside an indented block, file ends without newline.
    // The f-string sub-lexer must NOT inject an ExpressionStatementEnd.
    lexer_token_test(
        "fn f()\n    f\"{x}\"",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::FormattedStringStart(Box::default()),
            Token::Identifier,
            Token::FormattedStringEnd(Box::default()),
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_eof_no_newline_trait_method_signature() {
    // The exact pattern that triggered the original bug: a trait with an
    // abstract method (no body) in a file with no trailing newline.
    lexer_token_test(
        "trait T\n    fn foo()",
        vec![
            Token::Trait,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

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
