// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind, SyntaxErrors};
use crate::lexer::{Lexer, TokenSpan};

pub mod declarations;
pub mod expressions;
pub mod literals;
pub mod recovery;
pub mod statements;
pub mod types;
pub mod utils;

/// Maximum recursion depth allowed during parsing to prevent stack overflow DoS attacks.
pub const MAX_PARSE_DEPTH: usize = 256;

/// Recursive descent parser for Miri source code.
///
/// Consumes tokens from a `Lexer` and produces a `Program` AST.
/// Uses one token of lookahead for predictive parsing.
pub struct Parser<'source> {
    pub(super) lexer: &'source mut Lexer<'source>,
    pub(super) source: &'source str,
    pub(super) lookahead: Option<TokenSpan>,
    pub(super) depth: usize,
    /// End offset of the most recently consumed token.
    ///
    /// A statement's extent is only known once its last token is read, and the
    /// lookahead by then points past it. Recording the end as tokens are eaten
    /// lets [`Parser::statement`] close a span over everything the statement
    /// consumed, so a check that runs over a whole declaration has a location
    /// to report.
    pub(super) last_consumed_end: usize,
}

impl<'source> Parser<'source> {
    /// Creates a new parser from a lexer and source string.
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            lookahead: None,
            depth: 0,
            last_consumed_end: 0,
        }
    }

    /// Parses the token stream into a complete program AST, stopping at the
    /// first fault.
    ///
    /// Callers that rewrite a file from the tree — `fmt`, `patch`, `view` —
    /// cannot proceed past any fault, so a second one tells them nothing and
    /// the work of finding it is wasted. A caller that reports to a reader
    /// wants [`parse_all`](Self::parse_all).
    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self.program_from(0)
    }

    /// Parses the token stream, reporting at most one fault per top-level
    /// declaration.
    ///
    /// After a fault the parse resumes at the next line that opens a top-level
    /// declaration, abandoning the rest of the one that failed. That is what
    /// keeps a single fault from cascading: everything indented under the
    /// broken declaration is skipped rather than parsed out of context.
    pub fn parse_all(&mut self) -> Result<Program, SyntaxErrors> {
        match self.program_from(0) {
            Ok(program) => Ok(program),
            Err(first) => Err(self.faults_in_later_declarations(first)),
        }
    }

    /// Parses the declarations that begin at `offset`.
    fn program_from(&mut self, offset: usize) -> Result<Program, SyntaxError> {
        self.lexer.resume_at(offset);
        self.depth = 0;
        self.last_consumed_end = offset;
        self.lookahead = self.lexer.next().transpose()?;
        Ok(ast::program(self.statement_list()?))
    }

    /// Collects the faults in the declarations that follow `first`.
    ///
    /// Each round resumes strictly past the previous resume point, so the loop
    /// is bounded by the number of declarations in the source. Everything
    /// between the fault and the next boundary is abandoned rather than parsed
    /// out of context, which is what keeps one fault from cascading.
    fn faults_in_later_declarations(&mut self, first: SyntaxError) -> SyntaxErrors {
        let mut faults = SyntaxErrors::new(first);
        let mut resumed_at = 0;
        while let Some(offset) = self.next_declaration_start(resumed_at) {
            resumed_at = offset;
            match self.program_from(offset) {
                Ok(_) => break,
                Err(error) => faults.push(error),
            }
        }
        faults
    }

    /// The offset of the next declaration that opens a line, scanning forward
    /// from where the failed parse stopped. `None` at end of input.
    ///
    /// The scan reads tokens rather than source text, and that is what keeps it
    /// out of a string literal or a comment: the lexer produces each of those
    /// as one token, so a `fn` written inside a multi-line string is never seen
    /// as a declaration. A text scan for the same shape would resume in the
    /// middle of the literal and report a fault the file does not have.
    fn next_declaration_start(&mut self, after: usize) -> Option<usize> {
        while let Some((token, span)) = self.next_token_to_scan() {
            if span.start > after
                && recovery::opens_a_line(self.source, span.start)
                && recovery::opens_a_declaration(&token)
            {
                return Some(span.start);
            }
        }
        None
    }

    /// The next token the resynchronisation scan should look at: the one the
    /// failed parse stopped on, then whatever follows it.
    ///
    /// Lexer faults met while scanning are dropped. They lie inside the
    /// declaration the parse has already rejected, and a reader who fixes the
    /// fault that was reported gets them on the next run.
    fn next_token_to_scan(&mut self) -> Option<TokenSpan> {
        if let Some(pending) = self.lookahead.take() {
            return Some(pending);
        }
        loop {
            match self.lexer.next()? {
                Ok(token) => return Some(token),
                Err(_) => continue,
            }
        }
    }

    /// Enters a recursive-descent frame, rejecting input that nests deeper than
    /// `MAX_PARSE_DEPTH`. Every recursion cycle that can be driven arbitrarily
    /// deep by input alone — statements, the `expression` entry, and the
    /// operator rules that self-recurse without passing back through
    /// `expression` (prefix unary, postfix conditional) — must call this so a
    /// malformed program returns `RecursionLimitExceeded` instead of exhausting
    /// the native stack. Each successful call must be paired with `exit_recursion`.
    pub(super) fn enter_recursion(&mut self) -> Result<(), SyntaxError> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            self.depth -= 1;
            return Err(self.recursion_limit_error());
        }
        Ok(())
    }

    pub(super) fn exit_recursion(&mut self) {
        self.depth -= 1;
    }

    fn recursion_limit_error(&self) -> SyntaxError {
        let span = self
            .lookahead
            .as_ref()
            .map(|(_, s)| *s)
            .unwrap_or(Span::new(0, 0));
        SyntaxError::new(SyntaxErrorKind::RecursionLimitExceeded, span)
    }
}
