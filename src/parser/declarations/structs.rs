// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn struct_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Struct)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;

        let (fields, methods) = self.struct_body()?;

        if fields.is_empty() && methods.is_empty() {
            return Err(self.error_missing_members(SyntaxErrorKind::MissingStructMembers));
        }

        Ok(ast::struct_statement(
            name,
            generic_types,
            fields,
            methods,
            visibility,
        ))
    }

    /// Parses a struct body which may contain field declarations and method declarations.
    /// Accepts both inline (`: field type`) and block (indented) forms.
    fn struct_body(&mut self) -> Result<(Vec<Expression>, Vec<Statement>), SyntaxError> {
        let mut fields = vec![];
        let mut methods = vec![];

        match &self._lookahead {
            Some((Token::Colon, _)) => {
                // Inline form — only fields allowed inline
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self._lookahead.is_some() {
                    fields.push(self.struct_member_expression()?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        fields.push(self.struct_member_expression()?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form — may contain fields and/or method declarations
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        match &self._lookahead {
                            Some((Token::Fn, _))
                            | Some((Token::Async, _))
                            | Some((Token::Gpu, _)) => {
                                let stmt = self.function_declaration(MemberVisibility::Public)?;
                                methods.push(stmt);
                            }
                            _ => {
                                fields.push(self.struct_member_expression()?);
                            }
                        }
                        self.try_eat_expression_end();
                    }
                    self.eat_token(&Token::Dedent)?;
                }
            }
            _ => {
                return Err(self.error_unexpected_lookahead_token(
                    "either a colon for inline structs or an indentation for block structs",
                ));
            }
        }

        Ok((fields, methods))
    }

    pub(crate) fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_struct_member_type())?;
        Ok(ast::struct_member_expression(name, typ))
    }
}
