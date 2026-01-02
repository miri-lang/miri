// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use logos::Logos;
use std::collections::VecDeque;

use crate::error::syntax::{SyntaxError, SyntaxErrorKind};

pub mod formatted_string;
pub mod regex;
pub mod token;
pub mod utils;

pub use token::{RegexToken, Token, TokenSpan};
pub use utils::token_to_string;

use self::formatted_string::lex_formatted_string;
use self::regex::parse_regex_literal;

pub struct Lexer<'source> {
    inner: logos::Lexer<'source, Token>,
    source: &'source str,
    pending_tokens_stack: Vec<TokenSpan>,
    indent_stack: Vec<usize>, // stack of indent levels (in spaces)
    indent_level: usize,      // current indent level
    eof_handled: bool,
    paren_stack: Vec<usize>,          // stack of parenthesis levels
    bracket_stack: Vec<usize>,        // stack of square bracket levels
    curly_brace_stack: Vec<usize>,    // stack of curly brace levels
    previous_tokens: VecDeque<Token>, // keeps track of previous tokens, primarily for indentation handling
}

impl<'source> Iterator for Lexer<'source> {
    type Item = Result<TokenSpan, SyntaxError>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.generate_token();
        if let Some(Ok((token, _))) = &item {
            self.memorize_token(token.clone());
        }
        item
    }
}

impl<'source> Lexer<'source> {
    const MAX_PREVIOUS_TOKENS: usize = 2;

    pub fn new(source: &'source str) -> Self {
        Lexer {
            inner: Token::lexer(source),
            source,
            pending_tokens_stack: Vec::new(),
            indent_stack: vec![0],
            indent_level: 0,
            eof_handled: false,
            paren_stack: Vec::new(),
            bracket_stack: Vec::new(),
            curly_brace_stack: Vec::new(),
            previous_tokens: VecDeque::with_capacity(Self::MAX_PREVIOUS_TOKENS),
        }
    }

    fn generate_token(&mut self) -> Option<Result<TokenSpan, SyntaxError>> {
        loop {
            if let Some(item) = self.pending_tokens_stack.pop() {
                return Some(Ok(item));
            }

            let token = match self.inner.next() {
                Some(Ok(t)) => t,
                Some(Err(_)) => {
                    // This is where logos itself detects an error.
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidToken,
                        self.inner.span(),
                    )));
                }
                None => {
                    if !self.eof_handled {
                        self.eof_handled = true;
                        let source_len = self.source.len();

                        // Generate dedent tokens for all remaining indentation levels
                        while self.indent_stack.len() > 1 {
                            self.pending_tokens_stack
                                .push((Token::Dedent, source_len..source_len));
                            self.indent_stack.pop();
                        }

                        // Return the first pending dedent token if any
                        return self.pending_tokens_stack.pop().map(Ok);
                    }
                    return None;
                }
            };

            let span = self.inner.span();

            match token {
                Token::MultilineComment => {
                    if let Err(e) = self.lex_nested_comment() {
                        return Some(Err(e));
                    }
                    continue;
                }
                Token::Newline => {
                    if self.have_previous_tokens() {
                        if let Err(e) = self.lex_newline() {
                            return Some(Err(e));
                        }
                    }
                    continue;
                }
                Token::LParen => {
                    self.paren_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LParen, span)));
                }
                Token::RParen => {
                    self.paren_stack.pop();
                    return Some(Ok((Token::RParen, span)));
                }
                Token::LBracket => {
                    self.bracket_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LBracket, span)));
                }
                Token::RBracket => {
                    self.bracket_stack.pop();
                    return Some(Ok((Token::RBracket, span)));
                }
                Token::LBrace => {
                    self.curly_brace_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LBrace, span)));
                }
                Token::RBrace => {
                    self.curly_brace_stack.pop();
                    return Some(Ok((Token::RBrace, span)));
                }
                Token::SingleQuotedRegex | Token::DoubleQuotedRegex => {
                    let quote_char = if token == Token::SingleQuotedRegex {
                        '\''
                    } else {
                        '"'
                    };
                    match parse_regex_literal(&self.inner, quote_char) {
                        Ok(regex) => return Some(Ok((Token::Regex(regex), span))),
                        Err(e) => return Some(Err(e)),
                    }
                }
                Token::SingleQuotedString | Token::DoubleQuotedString => {
                    return Some(Ok((Token::String, span)));
                }
                Token::SingleQuotedFormattedString | Token::DoubleQuotedFormattedString => {
                    let quote_char = if token == Token::SingleQuotedFormattedString {
                        '\''
                    } else {
                        '"'
                    };
                    if let Err(e) = lex_formatted_string(
                        &mut self.inner,
                        &mut self.pending_tokens_stack,
                        quote_char,
                    ) {
                        return Some(Err(e));
                    }
                    continue;
                }
                Token::FloatOrRange => {
                    if let Err(e) = self.lex_float_or_range() {
                        return Some(Err(e));
                    }
                    continue;
                }
                Token::InvalidNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidNumberLiteral,
                        span,
                    )));
                }
                Token::InvalidBinaryNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidBinaryLiteral,
                        span,
                    )));
                }
                Token::InvalidHexNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidHexLiteral,
                        span,
                    )));
                }
                Token::InvalidOctalNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidOctalLiteral,
                        span,
                    )));
                }
                _ => return Some(Ok((token, span))),
            }
        }
    }

    fn lex_nested_comment(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        let mut depth = 1;
        let mut i = self.inner.span().end;

        while i + 1 < src.len() {
            let ch = &src[i..i + 2];
            match ch {
                "/*" => {
                    depth += 1;
                    i += 2;
                }
                "*/" => {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        let bump_len = i - self.inner.span().start - 2;
                        self.inner.bump(bump_len);
                        return Ok(());
                    }
                }
                _ => i += 1,
            }
        }

        Err(SyntaxError::new(
            SyntaxErrorKind::UnclosedMultilineComment,
            self.inner.span(),
        ))
    }

    fn lex_newline(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        let token_end = self.inner.span().end;
        let mut indent_len: usize = 0;
        let mut found_comment = false;
        let mut found_newline = false;

        // Look ahead from the end of the current newline token to calculate indentation.
        let mut lookahead_cursor = self.inner.span().end;

        // Count indentation on the next line
        while lookahead_cursor < src.len() {
            let ch = &src[lookahead_cursor..lookahead_cursor + 1];
            match ch {
                " " => indent_len += 1,
                "\t" => indent_len += 4, // Assuming tab width is 4
                "/" => {
                    if lookahead_cursor + 1 < src.len() {
                        let next_ch = &src[lookahead_cursor + 1..lookahead_cursor + 2];
                        if next_ch == "/" || next_ch == "*" {
                            found_comment = true;
                        }
                    }
                    break;
                }
                "\n" | "\r" => {
                    found_newline = true;
                    break;
                }
                _ => break,
            }
            lookahead_cursor += 1;
        }

        if !found_comment && !found_newline {
            // Handle indentation changes
            // SAFETY: indent_stack is initialized with [0] and never empty
            let last_indent = *self
                .indent_stack
                .last()
                .expect("Indent stack should not be empty");

            if indent_len > last_indent {
                // If we are not inside parentheses or brackets, treat as an indentation increase
                if self.is_outside_paired_tokens() {
                    // Indentation increase
                    self.push_indent(token_end, indent_len);
                } else if !self.paren_stack.is_empty()
                    && self.prev_tokens_match_function_declaration()
                {
                    // If this is a function declaration within function arguments, treat as an indentation increase
                    self.push_indent(token_end, indent_len);
                }
            } else if indent_len < last_indent {
                // Dedentation - must match a previous indentation level
                let mut found_matching_indent = false;

                for &level in self.indent_stack.iter() {
                    if level == indent_len {
                        found_matching_indent = true;
                        break;
                    }
                }

                if !found_matching_indent {
                    return Err(SyntaxError::new(
                        SyntaxErrorKind::IndentationMismatch,
                        token_end..token_end,
                    ));
                }

                // Pop indentation levels and generate Dedent tokens
                while indent_len < *self.indent_stack.last().unwrap() {
                    self.push_dedent(token_end);
                }
            }

            if self.is_expression_statement_end() {
                // If this is an expression statement end, return ExpressionStatementEnd token
                self.pending_tokens_stack
                    .push((Token::ExpressionStatementEnd, token_end..token_end));
            }
        }

        Ok(())
    }

    fn lex_float_or_range(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        let lookahead_cursor = self.inner.span().end;

        if lookahead_cursor < src.len() {
            let ch = &src[lookahead_cursor..lookahead_cursor + 1];
            if ch == "." {
                // It's a range
                let range_start = lookahead_cursor - 1;
                let range_end = lookahead_cursor + 1;

                if range_end < src.len() && &src[range_end..range_end + 1] == "=" {
                    // It's a range inclusive
                    self.pending_tokens_stack
                        .push((Token::RangeInclusive, range_start..(range_end + 1)));
                    self.inner.bump(2);
                } else {
                    self.pending_tokens_stack
                        .push((Token::Range, range_start..range_end));
                    self.inner.bump(1);
                }
                self.pending_tokens_stack
                    .push((Token::Int, self.inner.span().start..range_start));

                return Ok(());
            } else if ch.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                // It's a method call on an integer e.g. `1.to_string()`
                self.pending_tokens_stack
                    .push((Token::Dot, self.inner.span().end - 1..self.inner.span().end));
                self.pending_tokens_stack.push((
                    Token::Int,
                    self.inner.span().start..self.inner.span().end - 1,
                ));
                return Ok(());
            }
        }

        // It's a float
        self.pending_tokens_stack
            .push((Token::Float, self.inner.span()));

        Ok(())
    }

    fn memorize_token(&mut self, token: Token) {
        if self.previous_tokens.len() == Self::MAX_PREVIOUS_TOKENS {
            self.previous_tokens.pop_front();
        }
        self.previous_tokens.push_back(token);
    }

    fn have_previous_tokens(&self) -> bool {
        !self.previous_tokens.is_empty()
    }

    fn matches_previous_tokens(&self, tokens: &[Token]) -> bool {
        if tokens.len() > Self::MAX_PREVIOUS_TOKENS {
            panic!(
                "[Lexer] BUG: Trying to match {} previous tokens, but only {} allowed",
                tokens.len(),
                Self::MAX_PREVIOUS_TOKENS
            );
        }

        if self.previous_tokens.len() < tokens.len() {
            return false;
        }

        let start_index = self.previous_tokens.len() - tokens.len();
        self.previous_tokens
            .iter()
            .skip(start_index)
            .eq(tokens.iter())
    }

    fn match_previous_token(&self, token: Token) -> bool {
        self.previous_tokens.back().is_some_and(|t| *t == token)
    }

    fn prev_tokens_match_function_declaration(&self) -> bool {
        self.matches_previous_tokens(&[Token::RParen, Token::Identifier])
            || self.matches_previous_tokens(&[Token::RParen])
    }

    fn push_indent(&mut self, i: usize, indent_len: usize) {
        self.pending_tokens_stack.push((Token::Indent, i..i));
        self.indent_stack.push(indent_len);
        self.indent_level += 1;
    }

    fn push_dedent(&mut self, i: usize) {
        self.pending_tokens_stack.push((Token::Dedent, i..i));
        self.indent_stack.pop();
        self.indent_level -= 1;
    }

    fn is_outside_paired_tokens(&self) -> bool {
        self.paren_stack.is_empty()
            && self.bracket_stack.is_empty()
            && self.curly_brace_stack.is_empty()
    }

    fn is_inside_code_block(&self) -> bool {
        self.indent_level > 0
    }

    fn is_expression_statement_end(&self) -> bool {
        (self.is_outside_paired_tokens() || self.is_inside_code_block())
            && !self.match_previous_token(Token::ExpressionStatementEnd)
    }
}
