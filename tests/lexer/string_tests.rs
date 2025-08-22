// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;


#[test]
fn test_strings_with_escapes() {
    lexer_test(r#"'string with \' quote' "string with \" quote""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_empty_strings() {
    lexer_test("'' \"\"", vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_multiline_strings() {
    lexer_test("'line1\nline2' \"line1\nline2\"", vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_mixed_quotes_in_strings() {
    lexer_test(r#"'string with "double" quotes' "string with 'single' quotes""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_unicode_strings() {
    lexer_test(r#""Hello 世界" "🚀 rocket""#, vec![
        Token::DoubleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_string_escape_sequences() {
    lexer_test(r#"'line1\nline2\ttab' "quote\"inside""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_string_with_uncommon_escapes() {
    // Test escapes for backslash and different quote types
    lexer_test(r#""a \\ b" 'c \' d' "e \" f""#, vec![
        Token::DoubleQuotedString,
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_unclosed_string_literal() {
    // An unclosed string should likely be tokenized up to the end of the line
    // and not consume the rest of the file.
    lexer_error_test("'unclosed string", SyntaxErrorKind::InvalidToken);
}
