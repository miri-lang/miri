// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::ast_factory as ast;
use crate::lexer::{Lexer, TokenSpan};
use crate::syntax_error::{SyntaxError, SyntaxErrorKind};

pub mod declarations;
pub mod expressions;
pub mod literals;
pub mod statements;
pub mod types;
pub mod utils;

pub(crate) struct DeclarationBlockConfig<'a> {
    pub inline_error: &'a str,
    pub missing_members_error: SyntaxErrorKind,
}

pub struct Parser<'source> {
    pub(super) lexer: &'source mut Lexer<'source>,
    pub(super) source: &'source str,
    pub(super) _lookahead: Option<TokenSpan>,
}

impl<'source> Parser<'source> {
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
        }
    }

    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self._lookahead = self.lexer.next().transpose()?;
        self.program()
    }

    /*
        Program
            : StatementList
            ;
    */
    fn program(&mut self) -> Result<Program, SyntaxError> {
        let statements = self.statement_list()?;
        Ok(ast::program(statements))
    }
}
