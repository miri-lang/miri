// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::{
    error::syntax::SyntaxErrorKind,
    lexer::{RegexToken, Token},
};

use super::utils::{lexer_error_test, lexer_token_test, run_lexer_error_tests, run_lexer_tests};

#[test]
fn test_strings() {
    lexer_token_test(
        "'single quote' \"double quote\"",
        vec![Token::String, Token::String],
    );
}

#[test]
fn test_empty_strings() {
    lexer_token_test("'' \"\"", vec![Token::String, Token::String]);
}

#[test]
fn test_strings_with_escapes() {
    lexer_token_test(
        r#"'string with \' quote' "string with \" quote""#,
        vec![Token::String, Token::String],
    );
}

#[test]
fn test_string_with_uncommon_escapes() {
    // Test escapes for backslash and different quote types
    lexer_token_test(
        r#""a \\ b" 'c \' d' "e \" f""#,
        vec![Token::String, Token::String, Token::String],
    );
}

#[test]
fn test_multiline_strings() {
    lexer_token_test(
        "'line1\nline2' \"line1\nline2\"",
        vec![Token::String, Token::String],
    );
}

#[test]
fn test_mixed_quotes_in_strings() {
    lexer_token_test(
        r#"'string with "double" quotes' "string with 'single' quotes""#,
        vec![Token::String, Token::String],
    );
}

#[test]
fn test_unicode_strings() {
    lexer_token_test(
        r#""Hello 世界" "🚀 rocket""#,
        vec![Token::String, Token::String],
    );
}

#[test]
fn test_nested_strings() {
    lexer_token_test(r#"" \"inner\" 'inner' ""#, vec![Token::String]);

    lexer_token_test(r#"' \'inner\' "inner" '"#, vec![Token::String]);
}

#[test]
fn test_unclosed_string_literal() {
    // An unclosed string should likely be tokenized up to the end of the line
    // and not consume the rest of the file.
    lexer_error_test("'unclosed string", &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_string_with_complex_escapes() {
    lexer_token_test(r#""a \\\" b""#, vec![Token::String]);
    lexer_token_test(r#"'a \\\' b'"#, vec![Token::String]);
}

#[test]
fn test_f_string() {
    lexer_token_test(
        r#"f"val={x+1} and more {y+2}""#,
        vec![
            Token::FormattedStringStart(Box::new("val=".to_string())),
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::FormattedStringMiddle(Box::new(" and more ".to_string())),
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_empty() {
    lexer_token_test(
        r#"f"" f''"#,
        vec![
            Token::FormattedStringStart(Box::new("".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
            Token::FormattedStringStart(Box::new("".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_no_expressions() {
    lexer_token_test(
        r#"f"this is just a string""#,
        vec![
            Token::FormattedStringStart(Box::new("this is just a string".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_starts_with_expression() {
    lexer_token_test(
        r#"f"{x} starts here""#,
        vec![
            Token::FormattedStringStart(Box::new("".to_string())),
            Token::Identifier,
            Token::FormattedStringEnd(Box::new(" starts here".to_string())),
        ],
    );
}

#[test]
fn test_f_string_ends_with_expression() {
    lexer_token_test(
        r#"f"ends with {x}""#,
        vec![
            Token::FormattedStringStart(Box::new("ends with ".to_string())),
            Token::Identifier,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_adjacent_expressions() {
    lexer_token_test(
        r#"f"{x}{y}""#,
        vec![
            Token::FormattedStringStart(Box::new("".to_string())),
            Token::Identifier,
            Token::FormattedStringMiddle(Box::new("".to_string())),
            Token::Identifier,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_escaped_braces() {
    lexer_token_test(
        r#"f"Literal braces: \{ and \}""#,
        vec![
            Token::FormattedStringStart(Box::new("Literal braces: \\{ and \\}".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_escaped_braces_and_expression() {
    lexer_token_test(
        r#"f"\{ not code \} but {x} is""#,
        vec![
            Token::FormattedStringStart(Box::new("\\{ not code \\} but ".to_string())),
            Token::Identifier,
            Token::FormattedStringEnd(Box::new(" is".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_nested_braces_in_expression() {
    lexer_token_test(
        r#"f"A map: {{'key': 'value'}}""#,
        vec![
            Token::FormattedStringStart(Box::new("A map: ".to_string())),
            Token::LBrace,
            Token::String,
            Token::Colon,
            Token::String,
            Token::RBrace,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_single_quotes() {
    lexer_token_test(
        r#"f'hello {name}'"#,
        vec![
            Token::FormattedStringStart(Box::new("hello ".to_string())),
            Token::Identifier,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_string_literal_in_expression() {
    lexer_token_test(
        r#"f"path: {'/home/' + user}""#,
        vec![
            Token::FormattedStringStart(Box::new("path: ".to_string())),
            Token::String,
            Token::Plus,
            Token::Identifier,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_nested_string_literal() {
    lexer_error_test(
        r#"f"Greeting: {\"hello \" + name}""#,
        &SyntaxErrorKind::BackslashInFStringExpression,
    );
}

#[test]
fn test_f_string_error_unclosed_brace() {
    lexer_error_test(
        r#"f"unclosed {x""#,
        &SyntaxErrorKind::InvalidFormattedStringExpression,
    );
}

#[test]
fn test_f_string_with_nested_f_string() {
    lexer_token_test(
        r#"f"Outer value: {f'inner value: {x + 1}'}""#,
        vec![
            Token::FormattedStringStart(Box::new("Outer value: ".to_string())),
            // Start of inner f-string
            Token::FormattedStringStart(Box::new("inner value: ".to_string())),
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::FormattedStringEnd(Box::new("".to_string())),
            // End of outer f-string
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_regex_inside() {
    lexer_token_test(
        r#"f"The pattern is {re'a-z'i}""#,
        vec![
            Token::FormattedStringStart(Box::new("The pattern is ".to_string())),
            Token::Regex(Box::new(RegexToken {
                body: "a-z".to_string(),
                ignore_case: true,
                global: false,
                multiline: false,
                dot_all: false,
                unicode: false,
            })),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_complex_escapes_in_expression() {
    lexer_error_test(
        r#"f"Command: {\"echo \\\"hello world\\\"\"}""#,
        &SyntaxErrorKind::BackslashInFStringExpression,
    );
}

#[test]
fn test_f_string_with_escaped_backslash_before_quote() {
    lexer_error_test(
        r#"f"Path: {\"C:\\\\Users\\\\\"}""#,
        &SyntaxErrorKind::BackslashInFStringExpression,
    );
}

#[test]
fn test_f_string_with_regex() {
    lexer_error_test(
        r#"f"Pattern: {re'^\d+$'}""#,
        &SyntaxErrorKind::BackslashInFStringExpression,
    );
}

#[test]
fn test_f_string_with_empty_expression() {
    lexer_token_test(
        r#"f"Empty: {}""#,
        vec![
            Token::FormattedStringStart(Box::new("Empty: ".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_escaped_backslash_before_brace() {
    lexer_token_test(
        r#"f"Literal backslash: \\{1+1}""#,
        vec![
            Token::FormattedStringStart(Box::new("Literal backslash: \\\\".to_string())),
            Token::Int,
            Token::Plus,
            Token::Int,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_with_double_braces() {
    lexer_token_test(
        r#"f"Literal backslash: \\{{'a': 1}}""#,
        vec![
            Token::FormattedStringStart(Box::new("Literal backslash: \\\\".to_string())),
            Token::LBrace,
            Token::String,
            Token::Colon,
            Token::Int,
            Token::RBrace,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_keyword_in_formatted_string() {
    lexer_token_test(
        r#"f"The value is {if x: 1 else: 0}""#,
        vec![
            Token::FormattedStringStart(Box::new("The value is ".to_string())),
            Token::If,
            Token::Identifier,
            Token::Colon,
            Token::Int,
            Token::Else,
            Token::Colon,
            Token::Int,
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_f_string_error_unclosed_string() {
    run_lexer_error_tests(
        vec![r#"f"this is not closed"#, r#"f"unclosed with expr {x}"#],
        &SyntaxErrorKind::InvalidToken,
    );
}

#[test]
fn test_f_string_with_whitespace_expression() {
    lexer_token_test(
        r#"f"Whitespace: { }""#,
        vec![
            Token::FormattedStringStart(Box::new("Whitespace: ".to_string())),
            Token::FormattedStringEnd(Box::new("".to_string())),
        ],
    );
}

#[test]
fn test_string_and_identifier_boundaries() {
    run_lexer_tests(vec![
        (r#""hello"world"#, vec![Token::String, Token::Identifier]),
        (r#""value"if"#, vec![Token::String, Token::If]),
    ]);
}
