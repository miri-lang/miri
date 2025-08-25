// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;


#[test]
fn test_strings_with_escapes() {
    lexer_test(r#"'string with \' quote' "string with \" quote""#, vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_empty_strings() {
    lexer_test("'' \"\"", vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_multiline_strings() {
    lexer_test("'line1\nline2' \"line1\nline2\"", vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_mixed_quotes_in_strings() {
    lexer_test(r#"'string with "double" quotes' "string with 'single' quotes""#, vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_unicode_strings() {
    lexer_test(r#""Hello 世界" "🚀 rocket""#, vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_string_escape_sequences() {
    lexer_test(r#"'line1\nline2\ttab' "quote\"inside""#, vec![
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_string_with_uncommon_escapes() {
    // Test escapes for backslash and different quote types
    lexer_test(r#""a \\ b" 'c \' d' "e \" f""#, vec![
        Token::String,
        Token::String,
        Token::String,
    ]);
}

#[test]
fn test_nested_strings() {
    lexer_test(r#"" \"inner\" 'inner' ""#, vec![
        Token::String,
    ]);

    lexer_test(r#"' \'inner\' "inner" '"#, vec![
        Token::String,
    ]);
}

#[test]
fn test_unclosed_string_literal() {
    // An unclosed string should likely be tokenized up to the end of the line
    // and not consume the rest of the file.
    lexer_error_test("'unclosed string", SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_f_string() {
    lexer_test(
        r#"f"val={x+1} and more {y+2}""#,
        vec![
            Token::FormattedStringStart("val=".to_string()),
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::FormattedStringMiddle(" and more ".to_string()),
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_empty() {
    lexer_test(
        r#"f"" f''"#,
        vec![
            Token::FormattedStringStart("".to_string()),
            Token::FormattedStringStart("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_no_expressions() {
    lexer_test(
        r#"f"this is just a string""#,
        vec![
            Token::FormattedStringStart("this is just a string".to_string()),
        ],
    );
}

#[test]
fn test_f_string_starts_with_expression() {
    lexer_test(
        r#"f"{x} starts here""#,
        vec![
            Token::FormattedStringStart("".to_string()),
            Token::Identifier,
            Token::FormattedStringEnd(" starts here".to_string()),
        ],
    );
}

#[test]
fn test_f_string_ends_with_expression() {
    lexer_test(
        r#"f"ends with {x}""#,
        vec![
            Token::FormattedStringStart("ends with ".to_string()),
            Token::Identifier,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_adjacent_expressions() {
    lexer_test(
        r#"f"{x}{y}""#,
        vec![
            Token::FormattedStringStart("".to_string()),
            Token::Identifier,
            Token::FormattedStringMiddle("".to_string()),
            Token::Identifier,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_escaped_braces() {
    lexer_test(
        r#"f"Literal braces: \{ and \}""#,
        vec![
            Token::FormattedStringStart("Literal braces: \\{ and \\}".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_escaped_braces_and_expression() {
    // Test escaped brace next to a real expression
    lexer_test(
        r#"f"\{ not code \} but {x} is""#,
        vec![
            Token::FormattedStringStart("\\{ not code \\} but ".to_string()),
            Token::Identifier,
            Token::FormattedStringEnd(" is".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_nested_braces_in_expression() {
    // This tests if the lexer correctly handles balanced braces inside an expression.
    // The current implementation will likely fail this test, revealing a bug.
    lexer_test(
        r#"f"A map: {{'key': 'value'}}""#,
        vec![
            Token::FormattedStringStart("A map: ".to_string()),
            Token::LBrace,
            Token::String,
            Token::Colon,
            Token::String,
            Token::RBrace,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_single_quotes() {
    lexer_test(
        r#"f'hello {name}'"#,
        vec![
            Token::FormattedStringStart("hello ".to_string()),
            Token::Identifier,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_string_literal_in_expression() {
    lexer_test(
        r#"f"path: {'/home/' + user}""#,
        vec![
            Token::FormattedStringStart("path: ".to_string()),
            Token::String,
            Token::Plus,
            Token::Identifier,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_with_nested_string_literal() {
    // Tests that the parser correctly handles a regular string inside an f-string expression.
    lexer_test(
        r#"f"Greeting: {\"hello \" + name}""#,
        vec![
            Token::FormattedStringStart("Greeting: ".to_string()),
            Token::String,
            Token::Plus,
            Token::Identifier,
            Token::FormattedStringEnd("".to_string()),
        ],
    );
}

#[test]
fn test_f_string_error_unclosed_brace() {
    // The lexer should detect an unclosed brace in an f-string as an error.
    lexer_error_test(r#"f"unclosed {x""#, SyntaxErrorKind::InvalidFormattedStringExpression);
}

#[test]
fn test_string_with_complex_escapes() {
    lexer_test(r#""a \\\" b""#, vec![Token::String]);
    lexer_test(r#"'a \\\' b'"#, vec![Token::String]);
}
