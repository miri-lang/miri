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

const TAB_WIDTH: usize = 4;

/// One step of `generate_token`: emit a token, surface an error, or continue scanning.
enum Step {
    Emit(TokenSpan),
    Fail(SyntaxError),
    Continue,
}

impl From<Result<(), SyntaxError>> for Step {
    fn from(result: Result<(), SyntaxError>) -> Self {
        match result {
            Ok(()) => Step::Continue,
            Err(e) => Step::Fail(e),
        }
    }
}

/// What `dispatch_token` should do with a raw token from the inner logos lexer.
pub(crate) enum LexAction {
    ContinueWith(LexerWork),
    TrackOpen(BracketLevel),
    TrackClose(BracketLevel),
    Regex(char),
    PromoteToString,
    FormattedString(char),
    Invalid(SyntaxErrorKind),
    EmitAsIs,
}

/// Continuation work the lexer must run before yielding the next token.
pub(crate) enum LexerWork {
    NestedComment,
    Newline,
    FloatOrRange,
}

/// Bracketing categories tracked for indentation suppression.
pub(crate) enum BracketLevel {
    Paren,
    Bracket,
    Brace,
}

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
                Some(Err(_)) => return Some(Err(self.span_error(SyntaxErrorKind::InvalidToken))),
                None => return self.finalize_eof(),
            };
            let span = self.inner.span();

            match self.dispatch_token(token, span.start, span.end) {
                Step::Emit(item) => return Some(Ok(item)),
                Step::Fail(err) => return Some(Err(err)),
                Step::Continue => {}
            }
        }
    }

    fn dispatch_token(&mut self, token: Token, start: usize, end: usize) -> Step {
        match token.lex_action() {
            LexAction::ContinueWith(work) => match work {
                LexerWork::NestedComment => self.lex_nested_comment().into(),
                LexerWork::Newline => self.lex_newline_if_have_previous().into(),
                LexerWork::FloatOrRange => self.lex_float_or_range().into(),
            },
            LexAction::TrackOpen(level) => {
                self.bump_open(level);
                Step::Emit((token, Span::new(start, end)))
            }
            LexAction::TrackClose(level) => {
                self.bump_close(level);
                Step::Emit((token, Span::new(start, end)))
            }
            LexAction::Regex(quote) => self.dispatch_regex_literal(quote, start, end),
            LexAction::PromoteToString => Step::Emit((Token::String, Span::new(start, end))),
            LexAction::FormattedString(quote) => self.dispatch_formatted_string(quote),
            LexAction::Invalid(kind) => Step::Fail(error_at(kind, start, end)),
            LexAction::EmitAsIs => Step::Emit((token, Span::new(start, end))),
        }
    }

    fn lex_newline_if_have_previous(&mut self) -> Result<(), SyntaxError> {
        if self.have_previous_tokens() {
            self.lex_newline()?;
        }
        Ok(())
    }

    fn bump_open(&mut self, level: BracketLevel) {
        match level {
            BracketLevel::Paren => self.paren_level += 1,
            BracketLevel::Bracket => self.bracket_level += 1,
            BracketLevel::Brace => self.curly_brace_level += 1,
        }
    }

    fn bump_close(&mut self, level: BracketLevel) {
        match level {
            BracketLevel::Paren => self.paren_level = self.paren_level.saturating_sub(1),
            BracketLevel::Bracket => self.bracket_level = self.bracket_level.saturating_sub(1),
            BracketLevel::Brace => {
                self.curly_brace_level = self.curly_brace_level.saturating_sub(1)
            }
        }
    }

    fn dispatch_regex_literal(&mut self, quote: char, start: usize, end: usize) -> Step {
        match parse_regex_literal(&self.inner, quote) {
            Ok(regex) => Step::Emit((Token::Regex(regex), Span::new(start, end))),
            Err(e) => Step::Fail(e),
        }
    }

    fn dispatch_formatted_string(&mut self, quote: char) -> Step {
        match lex_formatted_string(&mut self.inner, &mut self.pending_tokens_stack, quote) {
            Ok(()) => Step::Continue,
            Err(e) => Step::Fail(e),
        }
    }

    fn span_error(&self, kind: SyntaxErrorKind) -> SyntaxError {
        let span = self.inner.span();
        SyntaxError::new(kind, Span::new(span.start, span.end))
    }

    fn finalize_eof(&mut self) -> Option<Result<TokenSpan, SyntaxError>> {
        if self.eof_handled {
            return None;
        }
        self.eof_handled = true;
        let source_len = self.source.len();

        // Guards on emitting a trailing ExpressionStatementEnd:
        //  - indent_stack > 1 skips sub-lexers that lex a single expression
        //    (e.g. formatted-string interpolations).
        //  - previous token != Indent skips empty indented blocks at EOF.
        let needs_ese = self.indent_stack.len() > 1
            && !self.match_previous_token(Token::Indent)
            && self.is_expression_statement_end();

        while self.indent_stack.len() > 1 {
            self.pending_tokens_stack
                .push((Token::Dedent, Span::new(source_len, source_len)));
            self.indent_stack.pop();
        }

        // Pushed last so it pops first (stack is LIFO).
        if needs_ese {
            self.pending_tokens_stack.push((
                Token::ExpressionStatementEnd,
                Span::new(source_len, source_len),
            ));
        }

        self.pending_tokens_stack.pop().map(Ok)
    }

    fn lex_nested_comment(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        // Byte view so multi-byte chars inside the comment do not panic the slice.
        let bytes = src.as_bytes();
        let mut depth = 1;
        let mut i = self.inner.span().end;

        while i + 1 < src.len() {
            match (bytes[i], bytes[i + 1]) {
                (b'/', b'*') => {
                    depth += 1;
                    i += 2;
                }
                (b'*', b'/') => {
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
        let token_end = self.inner.span().end;
        let scan = scan_indent(self.inner.source(), self.inner.span().end);

        if scan.found_comment || scan.found_newline {
            return Ok(());
        }

        self.apply_indent_change(scan.indent_len, token_end)?;

        if self.is_expression_statement_end() {
            self.pending_tokens_stack.push((
                Token::ExpressionStatementEnd,
                Span::new(token_end, token_end),
            ));
        }

        Ok(())
    }

    fn apply_indent_change(
        &mut self,
        indent_len: usize,
        token_end: usize,
    ) -> Result<(), SyntaxError> {
        // indent_stack is seeded with [0] and never empty under well-formed input,
        // but a malformed lookahead can deplete it — surface as IndentationMismatch.
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
            if self.is_outside_paired_tokens()
                || (self.paren_level > 0 && self.prev_tokens_match_function_declaration())
            {
                self.push_indent(token_end, indent_len);
            }
            return Ok(());
        }

        if indent_len < last_indent {
            if !self.indent_stack.contains(&indent_len) {
                return Err(SyntaxError::new(
                    SyntaxErrorKind::IndentationMismatch,
                    Span::new(token_end, token_end),
                ));
            }
            while let Some(&top) = self.indent_stack.last() {
                if indent_len < top {
                    self.push_dedent(token_end);
                } else {
                    break;
                }
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
                self.split_range(lookahead_cursor);
                return Ok(());
            }
            if ch.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                self.split_int_dot();
                return Ok(());
            }
        }

        self.pending_tokens_stack.push((
            Token::Float,
            Span::new(self.inner.span().start, self.inner.span().end),
        ));

        Ok(())
    }

    fn split_range(&mut self, lookahead_cursor: usize) {
        let src = self.inner.source();
        let range_start = lookahead_cursor - 1;
        let range_end = lookahead_cursor + 1;

        let int_span = Span::new(self.inner.span().start, range_start);
        if range_end < src.len() && &src[range_end..range_end + 1] == "=" {
            self.pending_tokens_stack
                .push((Token::RangeInclusive, Span::new(range_start, range_end + 1)));
            self.inner.bump(2);
        } else {
            self.pending_tokens_stack
                .push((Token::Range, Span::new(range_start, range_end)));
            self.inner.bump(1);
        }
        self.pending_tokens_stack.push((Token::Int, int_span));
    }

    fn split_int_dot(&mut self) {
        let end = self.inner.span().end;
        let start = self.inner.span().start;
        self.pending_tokens_stack
            .push((Token::Dot, Span::new(end - 1, end)));
        self.pending_tokens_stack
            .push((Token::Int, Span::new(start, end - 1)));
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
        debug_assert!(
            tokens.len() <= Self::MAX_PREVIOUS_TOKENS,
            "matches_previous_tokens: window {} exceeds MAX_PREVIOUS_TOKENS={}",
            tokens.len(),
            Self::MAX_PREVIOUS_TOKENS,
        );
        if tokens.len() > Self::MAX_PREVIOUS_TOKENS {
            return false;
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

fn error_at(kind: SyntaxErrorKind, start: usize, end: usize) -> SyntaxError {
    SyntaxError::new(kind, Span::new(start, end))
}

struct IndentScan {
    indent_len: usize,
    found_comment: bool,
    found_newline: bool,
}

fn scan_indent(src: &str, mut cursor: usize) -> IndentScan {
    let mut indent_len = 0;
    let mut found_comment = false;
    let mut found_newline = false;
    // Byte view so a multi-byte first char on the next line does not panic the slice.
    let bytes = src.as_bytes();

    while cursor < src.len() {
        match bytes[cursor] {
            b' ' => indent_len += 1,
            b'\t' => indent_len += TAB_WIDTH,
            b'/' => {
                if cursor + 1 < src.len() {
                    let next = bytes[cursor + 1];
                    if next == b'/' || next == b'*' {
                        found_comment = true;
                    }
                }
                break;
            }
            b'\n' | b'\r' => {
                found_newline = true;
                break;
            }
            _ => break,
        }
        cursor += 1;
    }

    IndentScan {
        indent_len,
        found_comment,
        found_newline,
    }
}
