// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;
use crate::parser::statements::{DeclarationBody, DeclarationMember};

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn struct_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Struct)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        let traits = self.inheritance_clause(&Token::Implements)?;

        let opening_comment = self.opening_comment();
        let body = self.struct_body()?;

        if body.entries.is_empty() && body.methods.is_empty() {
            return Err(
                self.error_missing_members(SyntaxErrorKind::MissingStructMembers, name.span)
            );
        }

        let mut statement = ast::struct_statement(
            name,
            generic_types,
            body.entries,
            body.methods,
            visibility,
            traits,
        );
        statement.trivia.opening_comment = opening_comment;
        statement.trivia.member_comments = body.member_comments;
        Ok(statement)
    }

    /// Parses a struct body which may contain field declarations and method declarations.
    /// Accepts both inline (`: field type`) and block (indented) forms.
    fn struct_body(&mut self) -> Result<DeclarationBody, SyntaxError> {
        let mut body = DeclarationBody::default();

        match &self.lookahead {
            Some((Token::Colon, _)) => {
                // Inline form — only fields allowed inline
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self.lookahead.is_some() {
                    body.entries.push(self.struct_member_expression()?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        body.entries.push(self.struct_member_expression()?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form — may contain fields and/or method declarations
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        self.declaration_member(&mut body, Self::struct_member)?;
                        self.try_eat_expression_end()?;
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

        Ok(body)
    }

    /// A method or a field of a block-form struct body.
    fn struct_member(&mut self) -> Result<DeclarationMember, SyntaxError> {
        match &self.lookahead {
            Some((Token::Fn, _)) | Some((Token::Async, _)) | Some((Token::Gpu, _)) => Ok(
                DeclarationMember::method(self.function_declaration(MemberVisibility::Public)?),
            ),
            _ => Ok(DeclarationMember::Entry(self.struct_member_expression()?)),
        }
    }

    pub(crate) fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let start = name.span.start;
        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_struct_member_type(name.span))?;
        let span = crate::error::syntax::Span::new(start, self.last_consumed_end);
        Ok(ast::struct_member_expression_with_span(name, typ, span))
    }
}
