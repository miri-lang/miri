// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::{Lexer, TokenSpan};

pub mod declarations;
pub mod expressions;
pub mod literals;
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
}

impl<'source> Parser<'source> {
    /// Creates a new parser from a lexer and source string.
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            lookahead: None,
            depth: 0,
        }
    }

    /// Parses the token stream into a complete program AST.
    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self.lookahead = self.lexer.next().transpose()?;
        self.program()
    }

    fn program(&mut self) -> Result<Program, SyntaxError> {
        let statements = self.statement_list()?;
        Ok(ast::program(statements))
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
