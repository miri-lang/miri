// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::{lexer_error_test, lexer_token_test, regex_token};

#[test]
fn test_simple_regex_literal() {
    lexer_token_test(r#"re"abc""#, vec![Token::Regex(regex_token("abc", ""))]);
}

#[test]
fn test_regex_with_all_flags() {
    lexer_token_test(
        r#"re"[a-z]+"igmsu"#,
        vec![Token::Regex(regex_token("[a-z]+", "igmsu"))],
    );
}

#[test]
fn test_regex_with_some_flags() {
    lexer_token_test(
        r#"re"^\d+$"im"#,
        vec![Token::Regex(regex_token("^\\d+$", "im"))],
    );
}

#[test]
fn test_regex_with_escaped_quotes_and_slashes() {
    lexer_token_test(
        r#"re"a\"b\\c""#,
        vec![Token::Regex(regex_token("a\\\"b\\\\c", ""))],
    );
}

#[test]
fn test_empty_regex() {
    lexer_token_test(r#"re""g"#, vec![Token::Regex(regex_token("", "g"))]);
}

#[test]
fn test_regex_is_not_a_string() {
    // Ensure re"..." is tokenized differently from a normal string followed by an identifier.
    lexer_token_test(
        "re\"abc\" \"abc\"g",
        vec![
            Token::Regex(regex_token("abc", "")),
            Token::String,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_regex_with_invalid_flags() {
    // The lexer should parse the valid flags and treat the rest as a separate token.
    lexer_token_test(
        r#"re"abc"ixyz"#,
        vec![
            Token::Regex(regex_token("abc", "i")),
            Token::Identifier, // "xyz"
        ],
    );
}

#[test]
fn test_regex_in_expression() {
    lexer_token_test(
        r#"let pattern = re"^\w+$"i"#,
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Regex(regex_token("^\\w+$", "i")),
        ],
    );
}

#[test]
fn test_error_unclosed_regex() {
    // An unclosed regex should be treated as an invalid token, similar to an unclosed string.
    lexer_error_test(r#"re"abc"#, &SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_error_regex_without_re_prefix() {
    // This should be parsed as a string followed by an identifier.
    lexer_token_test(r#""[a-z]+"g"#, vec![Token::String, Token::Identifier]);
}

#[test]
fn test_single_quoted_regex_literals() {
    lexer_token_test(
        r#"re'abc' re'a\'b'i re''"#,
        vec![
            Token::Regex(regex_token("abc", "")),
            Token::Regex(regex_token("a\\'b", "i")),
            Token::Regex(regex_token("", "")),
        ],
    );
}

#[test]
fn test_regex_with_various_escapes() {
    lexer_token_test(
        r#"re"line\n\t\{[0-9]+\}""#,
        vec![Token::Regex(regex_token("line\\n\\t\\{[0-9]+\\}", ""))],
    );
}

#[test]
fn test_regex_with_repeated_flags() {
    // The lexer should just set the flag to true once, behavior is idempotent.
    lexer_token_test(
        r#"re"abc"iig"#,
        vec![Token::Regex(regex_token("abc", "ig"))],
    );
}

#[test]
fn test_error_unclosed_regex_with_flags() {
    // An unclosed regex should fail even if valid flags are present.
    // Logos will fail to match the regex token, see `re` as an identifier,
    // and then see an unclosed string.
    lexer_error_test(r#"re"unclosed'i"#, &SyntaxErrorKind::InvalidToken);
}
