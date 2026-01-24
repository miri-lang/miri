// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::{Lexer, TokenSpan};

pub mod declarations;
pub mod expressions;
pub mod literals;
pub mod statements;
pub mod types;
pub mod utils;

/// Configuration for parsing declaration blocks (structs, enums, etc.).
pub(crate) struct DeclarationBlockConfig<'a> {
    pub inline_error: &'a str,
    pub missing_members_error: SyntaxErrorKind,
}

/// Recursive descent parser for Miri source code.
///
/// Consumes tokens from a `Lexer` and produces a `Program` AST.
/// Uses one token of lookahead for predictive parsing.
pub struct Parser<'source> {
    pub(super) lexer: &'source mut Lexer<'source>,
    pub(super) source: &'source str,
    pub(super) _lookahead: Option<TokenSpan>,
}

impl<'source> Parser<'source> {
    /// Creates a new parser from a lexer and source string.
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
        }
    }

    /// Parses the token stream into a complete program AST.
    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self._lookahead = self.lexer.next().transpose()?;
        self.program()
    }

    fn program(&mut self) -> Result<Program, SyntaxError> {
        let statements = self.statement_list()?;
        Ok(ast::program(statements))
    }
}
