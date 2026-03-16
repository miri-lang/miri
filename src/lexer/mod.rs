// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use logos::Logos;

use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};

pub mod formatted_string;
pub mod regex;
pub mod token;
pub mod utils;

pub use token::{RegexToken, Token, TokenSpan};
pub use utils::token_to_string;

use self::formatted_string::lex_formatted_string;
use self::regex::parse_regex_literal;

/// Indentation-aware lexer for Miri source code.
///
/// Wraps a `logos` lexer and adds indentation tracking, producing
/// synthetic `Indent`/`Dedent` tokens for significant whitespace.
pub struct Lexer<'source> {
    inner: logos::Lexer<'source, Token>,
    source: &'source str,
    pending_tokens_stack: Vec<TokenSpan>,
    indent_stack: Vec<usize>,
    indent_level: usize,
    eof_handled: bool,
    paren_level: usize,
    bracket_level: usize,
    curly_brace_level: usize,
    previous_tokens: [Option<Token>; 2],
    previous_tokens_count: usize,
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

    /// Creates a new lexer for the given source string.
    pub fn new(source: &'source str) -> Self {
        Lexer {
            inner: Token::lexer(source),
            source,
            pending_tokens_stack: Vec::new(),
            indent_stack: vec![0],
            indent_level: 0,
            eof_handled: false,
            paren_level: 0,
            bracket_level: 0,
            curly_brace_level: 0,
            previous_tokens: [None, None],
            previous_tokens_count: 0,
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
                        Span::new(self.inner.span().start, self.inner.span().end),
                    )));
                }
                None => {
                    if !self.eof_handled {
                        self.eof_handled = true;
                        let source_len = self.source.len();

                        while self.indent_stack.len() > 1 {
                            self.pending_tokens_stack
                                .push((Token::Dedent, Span::new(source_len, source_len)));
                            self.indent_stack.pop();
                        }

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
                    self.paren_level += 1;
                    return Some(Ok((Token::LParen, Span::new(span.start, span.end))));
                }
                Token::RParen => {
                    self.paren_level = self.paren_level.saturating_sub(1);
                    return Some(Ok((Token::RParen, Span::new(span.start, span.end))));
                }
                Token::LBracket => {
                    self.bracket_level += 1;
                    return Some(Ok((Token::LBracket, Span::new(span.start, span.end))));
                }
                Token::RBracket => {
                    self.bracket_level = self.bracket_level.saturating_sub(1);
                    return Some(Ok((Token::RBracket, Span::new(span.start, span.end))));
                }
                Token::LBrace => {
                    self.curly_brace_level += 1;
                    return Some(Ok((Token::LBrace, Span::new(span.start, span.end))));
                }
                Token::RBrace => {
                    self.curly_brace_level = self.curly_brace_level.saturating_sub(1);
                    return Some(Ok((Token::RBrace, Span::new(span.start, span.end))));
                }
                Token::SingleQuotedRegex | Token::DoubleQuotedRegex => {
                    let quote_char = if token == Token::SingleQuotedRegex {
                        '\''
                    } else {
                        '"'
                    };
                    match parse_regex_literal(&self.inner, quote_char) {
                        Ok(regex) => {
                            return Some(Ok((Token::Regex(regex), Span::new(span.start, span.end))))
                        }
                        Err(e) => return Some(Err(e)),
                    }
                }
                Token::SingleQuotedString | Token::DoubleQuotedString => {
                    return Some(Ok((Token::String, Span::new(span.start, span.end))));
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
                        Span::new(span.start, span.end),
                    )));
                }
                Token::InvalidBinaryNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidBinaryLiteral,
                        Span::new(span.start, span.end),
                    )));
                }
                Token::InvalidHexNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidHexLiteral,
                        Span::new(span.start, span.end),
                    )));
                }
                Token::InvalidOctalNumber => {
                    return Some(Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidOctalLiteral,
                        Span::new(span.start, span.end),
                    )));
                }
                _ => return Some(Ok((token, Span::new(span.start, span.end)))),
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
            Span::new(self.inner.span().start, self.inner.span().end),
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
            // SAFETY: indent_stack should be initialized with [0] and never empty.
            // However, malformed input could potentially deplete the stack.
            let last_indent = match self.indent_stack.last() {
                Some(&indent) => indent,
                None => {
                    return Err(SyntaxError::new(
                        SyntaxErrorKind::IndentationMismatch,
                        Span::new(token_end, token_end),
                    ));
                }
            };

            if indent_len > last_indent {
                // If we are not inside parentheses or brackets, treat as an indentation increase
                if self.is_outside_paired_tokens() {
                    // Indentation increase
                    self.push_indent(token_end, indent_len);
                } else if self.paren_level > 0 && self.prev_tokens_match_function_declaration() {
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
                        Span::new(token_end, token_end),
                    ));
                }

                // Pop indentation levels and generate Dedent tokens
                while let Some(&last_indent) = self.indent_stack.last() {
                    if indent_len < last_indent {
                        self.push_dedent(token_end);
                    } else {
                        break;
                    }
                }
            }

            if self.is_expression_statement_end() {
                // If this is an expression statement end, return ExpressionStatementEnd token
                self.pending_tokens_stack.push((
                    Token::ExpressionStatementEnd,
                    Span::new(token_end, token_end),
                ));
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
                        .push((Token::RangeInclusive, Span::new(range_start, range_end + 1)));
                    self.inner.bump(2);
                } else {
                    self.pending_tokens_stack
                        .push((Token::Range, Span::new(range_start, range_end)));
                    self.inner.bump(1);
                }
                self.pending_tokens_stack
                    .push((Token::Int, Span::new(self.inner.span().start, range_start)));

                return Ok(());
            } else if ch.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                // It's a method call on an integer e.g. `1.to_string()`
                self.pending_tokens_stack.push((
                    Token::Dot,
                    Span::new(self.inner.span().end - 1, self.inner.span().end),
                ));
                self.pending_tokens_stack.push((
                    Token::Int,
                    Span::new(self.inner.span().start, self.inner.span().end - 1),
                ));
                return Ok(());
            }
        }

        // It's a float
        self.pending_tokens_stack.push((
            Token::Float,
            Span::new(self.inner.span().start, self.inner.span().end),
        ));

        Ok(())
    }

    fn memorize_token(&mut self, token: Token) {
        if self.previous_tokens_count == Self::MAX_PREVIOUS_TOKENS {
            self.previous_tokens[0] = self.previous_tokens[1].take();
            self.previous_tokens[1] = Some(token);
        } else {
            self.previous_tokens[self.previous_tokens_count] = Some(token);
            self.previous_tokens_count += 1;
        }
    }

    fn have_previous_tokens(&self) -> bool {
        self.previous_tokens_count > 0
    }

    fn matches_previous_tokens(&self, tokens: &[Token]) -> bool {
        if tokens.len() > Self::MAX_PREVIOUS_TOKENS {
            panic!(
                "[Lexer] BUG: Trying to match {} previous tokens, but only {} allowed",
                tokens.len(),
                Self::MAX_PREVIOUS_TOKENS
            );
        }

        if self.previous_tokens_count < tokens.len() {
            return false;
        }

        let start_index = self.previous_tokens_count - tokens.len();
        for (i, token) in tokens.iter().enumerate() {
            if self.previous_tokens[start_index + i].as_ref() != Some(token) {
                return false;
            }
        }
        true
    }

    fn match_previous_token(&self, token: Token) -> bool {
        if self.previous_tokens_count == 0 {
            return false;
        }
        self.previous_tokens[self.previous_tokens_count - 1].as_ref() == Some(&token)
    }

    fn prev_tokens_match_function_declaration(&self) -> bool {
        self.matches_previous_tokens(&[Token::RParen, Token::Identifier])
            || self.matches_previous_tokens(&[Token::RParen])
    }

    fn push_indent(&mut self, i: usize, indent_len: usize) {
        self.pending_tokens_stack
            .push((Token::Indent, Span::new(i, i)));
        self.indent_stack.push(indent_len);
        self.indent_level += 1;
    }

    fn push_dedent(&mut self, i: usize) {
        self.pending_tokens_stack
            .push((Token::Dedent, Span::new(i, i)));
        self.indent_stack.pop();
        self.indent_level -= 1;
    }

    fn is_outside_paired_tokens(&self) -> bool {
        self.paren_level == 0 && self.bracket_level == 0 && self.curly_brace_level == 0
    }

    fn is_inside_code_block(&self) -> bool {
        self.indent_level > 0
    }

    fn is_expression_statement_end(&self) -> bool {
        (self.is_outside_paired_tokens() || self.is_inside_code_block())
            && !self.match_previous_token(Token::ExpressionStatementEnd)
    }
}
