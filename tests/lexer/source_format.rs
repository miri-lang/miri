// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::{lexer::Token, syntax_error::SyntaxErrorKind};
use std::vec;

#[test]
fn test_windows_line_endings_crlf() {
    lexer_test(
        "if x > 0\r\n    print(x)\r\nelse\r\n    print(0)",
        vec![
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::RParen,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_mixed_line_endings_lf_and_crlf() {
    lexer_test(
        "if x > 0\n    print(x)\r\nelse\n    print(0)",
        vec![
            Token::If,
            Token::Identifier,
            Token::GreaterThan,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Else,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::RParen,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_tab_indentation() {
    // The lexer should correctly interpret tabs as a multiple of spaces (e.g., 4).
    lexer_test(
        "fn my_func()\n\tprint(\"hello\")\n\tlet y = 1",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::String,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_mixed_spaces_and_tabs_indentation_error() {
    // Mixing tabs and spaces for indentation is a common source of errors.
    let code = "if true\n  let x = 1\n\tif true\n   print(x)";
    lexer_error_test(code, &SyntaxErrorKind::IndentationMismatch);
}

#[test]
fn test_unicode_in_strings_and_comments() {
    lexer_test(
        "// Коментар українською\nlet greeting = \"Слава Україні!\"",
        vec![Token::Let, Token::Identifier, Token::Assign, Token::String],
    );
}

#[test]
fn test_unicode_identifiers_are_not_supported() {
    lexer_error_test("let змінна = 1", &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_wide_range_of_unicode_in_strings_and_comments() {
    // The lexer should handle various Unicode characters from different scripts
    // within strings and comments without errors. The regexes for strings and
    // comments are byte-oriented and should not break on multi-byte characters.
    lexer_test(
        "// Ελληνικό σχόλιο\nlet s = \"你好世界\" // CJK\n// Emoji: ✨",
        vec![Token::Let, Token::Identifier, Token::Assign, Token::String],
    );
}

#[test]
fn test_invalid_utf8_replacement_character() {
    // If a non-UTF8 file is incorrectly forced into a Rust string,
    // invalid byte sequences become the Unicode replacement character '' (U+FFFD).
    // The lexer should treat this character as an invalid token, as it's not part
    // of any valid token definition.
    let invalid_utf8_string = "let x = ;";
    lexer_error_test(invalid_utf8_string, &SyntaxErrorKind::InvalidToken);
}
