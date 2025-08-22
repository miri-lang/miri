// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::{lexer::{Lexer, RegexToken, Token}, syntax_error::SyntaxErrorKind};


pub fn lexer_test(input: &str, expected: Vec<Token>) {
    let lexer = Lexer::new(input);
    let tokens: Vec<Token> = lexer.map(|result| result.unwrap().0).collect();
    assert_eq!(tokens, expected);
}

pub fn lexer_error_test(input: &str, expected_kind: SyntaxErrorKind) {
    let lexer = Lexer::new(input);
    let results: Vec<_> = lexer.collect();

    let error = results.iter().find_map(|res| res.as_ref().err().cloned());

    assert!(error.is_some(), "Expected a lexer error, but it succeeded without errors.");
    assert_eq!(error.unwrap().kind, expected_kind, "Lexer produced an error of the wrong kind.");
}

pub fn regex_token(body: &str, flags: &str) -> RegexToken {
    RegexToken {
        body: body.to_string(),
        ignore_case: flags.contains('i'),
        global: flags.contains('g'),
        multiline: flags.contains('m'),
        dot_all: flags.contains('s'),
        unicode: flags.contains('u'),
    }
}
