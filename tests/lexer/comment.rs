// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::{error::syntax::SyntaxErrorKind, lexer::Token};

use super::utils::{lexer_error_test, lexer_token_test};

#[test]
fn test_inline_comments() {
    lexer_token_test(
        r#"
var x = 10 // simple inline comment

print('Hello') // 👋 this is a friendly comment

use System.Math // use System.Math // with another comment inside

x = x + 1 // math: x becomes x + 1
"#,
        vec![
            Token::Var,
            Token::Identifier,
            Token::Assign,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::LParen,
            Token::String,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Use,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::Plus,
            Token::Int,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_multiline_comments() {
    lexer_token_test(
        r#"
/**/

/* This is a single-line comment */

/*****************************************/

/* This is a basic
multiline comment
spanning three lines */
let some = "code"

/* Multiline comment with code inside:
var a = 5
print('ignored!')
*/

fn func() int: 10 + 10

/***
/* 
  /* nested */ 
*/ 
***/

/*

  |\_/|
  ( o.o )   <- Cat!
  > ^ <

This is a comment with ASCII art.

Symbols: /* nested? */ < > & ^ ~
*/

print("Hello") /* inline comment */
"#,
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::String,
            Token::ExpressionStatementEnd,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::Identifier,
            Token::Colon,
            Token::Int,
            Token::Plus,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::LParen,
            Token::String,
            Token::RParen,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_deeply_nested_comments() {
    lexer_token_test(
        "before /* outer /* inner /* deepest */ inner */ outer */ after",
        vec![Token::Identifier, Token::Identifier],
    );
}

#[test]
fn test_unclosed_nested_comment() {
    lexer_error_test(
        "/* outer /* inner */ still open",
        &SyntaxErrorKind::UnclosedMultilineComment,
    );
}

#[test]
fn test_comment_with_code_like_content() {
    lexer_token_test("/* func(): if else */ real_code", vec![Token::Identifier]);
}

#[test]
fn test_comment_at_eof() {
    lexer_token_test("code // comment with no newline", vec![Token::Identifier]);
}

#[test]
fn test_nested_comments_with_strings() {
    lexer_token_test(
        r#"/* outer /* "string inside comment" */ outer */ code"#,
        vec![Token::Identifier],
    );
}

#[test]
fn test_multiline_comment_at_eof() {
    lexer_token_test("code /* comment */", vec![Token::Identifier]);
    lexer_token_test("/* comment */", vec![]);
}

#[test]
fn test_unclosed_comment_at_eof() {
    lexer_error_test(
        "code /* unclosed",
        &SyntaxErrorKind::UnclosedMultilineComment,
    );
    lexer_error_test("/*", &SyntaxErrorKind::UnclosedMultilineComment);
}

#[test]
fn test_comment_markers_inside_strings() {
    lexer_token_test(
        r#"let s1 = "This is not a // comment""#,
        vec![Token::Let, Token::Identifier, Token::Assign, Token::String],
    );
    lexer_token_test(
        r#"let s2 = "This is not a /* comment */""#,
        vec![Token::Let, Token::Identifier, Token::Assign, Token::String],
    );
}

#[test]
fn test_multiline_comment_with_multibyte_chars() {
    // Regression: previously panicked because lex_nested_comment byte-sliced
    // `&src[i..i+2]` across UTF-8 boundaries.
    lexer_token_test("/* 中 */ x", vec![Token::Identifier]);
    lexer_token_test("/* 🎯 nested /* 中 */ */ y", vec![Token::Identifier]);
}

#[test]
fn test_malformed_comment_delimiters() {
    // A lone closing comment delimiter is just a star and a slash, not a comment.
    lexer_token_test(
        "a */ b",
        vec![
            Token::Identifier,
            Token::Star,
            Token::Slash,
            Token::Identifier,
        ],
    );
    // A space breaks the opening delimiter.
    lexer_token_test(
        "a / * b",
        vec![
            Token::Identifier,
            Token::Slash,
            Token::Star,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_inline_comments_buffered_not_emitted() {
    use miri::lexer::Lexer;

    let source = r#"var x = 10 // inline comment
print('hi')"#;

    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        match result {
            Ok((token, _)) => tokens.push(token),
            Err(e) => panic!("Lexer error: {:?}", e),
        }
    }

    // Verify that no inline comment tokens appear in the stream
    assert!(
        !tokens.iter().any(|t| matches!(t, Token::InlineComment)),
        "InlineComment token should not be emitted"
    );

    // Verify that comments were buffered
    let comments = lexer.take_trailing_comments();
    assert_eq!(comments.len(), 1, "Should have 1 buffered comment");
    assert_eq!(comments[0].text, "// inline comment");
    assert!(
        lexer.take_leading_comments().is_empty(),
        "a comment after code is not also a leading one"
    );
}

#[test]
fn test_leading_comment_identification() {
    use miri::lexer::Lexer;

    let source = r#"
// leading comment
var x = 10"#;

    let mut lexer = Lexer::new(source);
    while let Some(result) = lexer.next() {
        let _ = result;
    }

    let comments = lexer.take_leading_comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "// leading comment");
    assert!(
        lexer.take_trailing_comments().is_empty(),
        "a comment starting its own line is not a trailing one"
    );
}

#[test]
fn test_multiple_inline_comments_buffering() {
    use miri::lexer::Lexer;

    let source = r#"var x = 10 // first
var y = 20 // second
// third (leading)
z = 30"#;

    let mut lexer = Lexer::new(source);
    while let Some(result) = lexer.next() {
        let _ = result;
    }

    let trailing = lexer.take_trailing_comments();
    let leading = lexer.take_leading_comments();
    let trailing_text: Vec<&str> = trailing.iter().map(|c| c.text.as_str()).collect();
    let leading_text: Vec<&str> = leading.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(trailing_text, vec!["// first", "// second"]);
    assert_eq!(leading_text, vec!["// third (leading)"]);
}

#[test]
fn test_token_stream_unchanged_with_and_without_comments() {
    use miri::lexer::Lexer;

    let with_comments = r#"var x = 10 // comment
print('hi')"#;

    let without_comments = r#"var x = 10
print('hi')"#;

    let lexer1 = Lexer::new(with_comments);
    let results1: Result<Vec<_>, _> = lexer1.map(|r| r.map(|(t, _)| t)).collect();
    let tokens1 = results1.expect("Lexing should succeed");

    let lexer2 = Lexer::new(without_comments);
    let results2: Result<Vec<_>, _> = lexer2.map(|r| r.map(|(t, _)| t)).collect();
    let tokens2 = results2.expect("Lexing should succeed");

    assert_eq!(
        tokens1, tokens2,
        "Token stream should be identical with and without comments"
    );
}
