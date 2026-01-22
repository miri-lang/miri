// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::token::{RegexToken, Token};
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use logos::Lexer;

pub fn parse_regex_literal(
    lexer: &Lexer<Token>,
    quote_character: char,
) -> Result<RegexToken, SyntaxError> {
    let slice = lexer.slice(); // Example: re"\d+"ig
    let without_prefix = &slice[2..]; // remove `re`

    let (start, end) = match (
        without_prefix.find(quote_character),
        without_prefix.rfind(quote_character),
    ) {
        (Some(s), Some(e)) if s != e => (s, e),
        _ => {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidRegexLiteral,
                lexer.span(),
            ));
        }
    };

    let regex_body = &without_prefix[start + 1..end];
    let flags = &without_prefix[end + 1..]; // everything after the final quote

    let mut regex = RegexToken {
        body: regex_body.to_string(),
        ignore_case: false,
        global: false,
        multiline: false,
        dot_all: false,
        unicode: false,
    };

    for flag in flags.chars() {
        match flag {
            'i' => regex.ignore_case = true,
            'g' => regex.global = true,
            'm' => regex.multiline = true,
            's' => regex.dot_all = true,
            'u' => regex.unicode = true,
            _ => {}
        }
    }

    Ok(regex)
}
