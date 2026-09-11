// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Restarting the lexer part-way through a file.
//!
//! The parser resumes at a top-level declaration after a syntax fault, which
//! only works if the restart drops the state the abandoned region left behind
//! and keeps reporting positions in whole-file coordinates.

use miri::error::syntax::Span;
use miri::lexer::{Lexer, Token};

/// Every token the lexer yields from `offset` to end of input, faults dropped.
fn tokens_from(source: &str, offset: usize) -> Vec<(Token, Span)> {
    let mut lexer = Lexer::new(source);
    lexer.resume_at(offset);
    lexer.flatten().collect()
}

/// The byte offset of `needle` in `source`.
fn offset_of(source: &str, needle: &str) -> usize {
    source
        .find(needle)
        .expect("the needle must be in the source")
}

#[test]
fn a_resumed_span_is_still_a_whole_file_offset() {
    let source = "fn alpha()\n    return 1\n\nfn beta()\n    return 2\n";
    let start = offset_of(source, "fn beta");
    let tokens = tokens_from(source, start);

    let (token, span) = tokens.first().expect("the resumed stream must yield");
    assert_eq!(*token, Token::Fn);
    assert_eq!(
        span.start, start,
        "spans must not be relative to the resume"
    );
    assert_eq!(&source[span.start..span.end], "fn");
}

#[test]
fn a_resume_reads_nothing_from_before_the_offset() {
    let source = "fn alpha()\n    return 1\n\nfn beta()\n    return 2\n";
    let tokens = tokens_from(source, offset_of(source, "fn beta"));

    let first_return = tokens
        .iter()
        .find(|(token, _)| *token == Token::Return)
        .expect("the resumed stream must reach the body");
    assert!(
        first_return.1.start > offset_of(source, "fn beta"),
        "the first `return` after the resume must be beta's, not alpha's"
    );
    assert_eq!(
        tokens
            .iter()
            .filter(|(token, _)| *token == Token::Return)
            .count(),
        1
    );
}

#[test]
fn a_resume_drops_a_bracket_the_abandoned_region_left_open() {
    let source = "fn broken(\n\nfn after()\n    return 1\n";
    let mut lexer = Lexer::new(source);
    // Read up to and including the `(`, which is never closed.
    for _ in 0..3 {
        let _ = lexer.next();
    }
    lexer.resume_at(offset_of(source, "fn after"));
    let tokens: Vec<Token> = lexer.flatten().map(|(token, _)| token).collect();

    assert!(
        tokens.contains(&Token::Indent),
        "an inherited open bracket would suppress indentation for the rest of \
         the file, leaving the body unblocked: {tokens:?}"
    );
}

#[test]
fn a_resume_past_the_end_of_input_yields_nothing_but_layout() {
    let source = "fn alpha()\n    return 1\n";
    let tokens = tokens_from(source, source.len());

    assert!(
        tokens
            .iter()
            .all(|(token, _)| matches!(token, Token::Dedent | Token::ExpressionStatementEnd)),
        "a resume at end of input must yield no code tokens: {tokens:?}"
    );
}

#[test]
fn a_resume_inside_a_character_resumes_at_end_of_input_instead() {
    let source = "fn alpha()\n    let s = \"日本語\"\n";
    let inside = offset_of(source, "日") + 1;
    assert!(!source.is_char_boundary(inside));

    let tokens = tokens_from(source, inside);

    assert!(
        tokens
            .iter()
            .all(|(token, _)| matches!(token, Token::Dedent | Token::ExpressionStatementEnd)),
        "an offset that would split a code point must not be lexed from: {tokens:?}"
    );
}
