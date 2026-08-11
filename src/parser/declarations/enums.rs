// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn enum_statement(
        &mut self,
        visibility: MemberVisibility,
        mut attributes: Vec<Attribute>,
    ) -> Result<Statement, SyntaxError> {
        // The keyword spelling becomes the attribute it is an alias for, so the
        // registry and every consumer see one representation. The spelling is
        // retained only so the type checker can report the deprecation.
        if self.match_lookahead_type(|t| t == &Token::MustUse) {
            let span = match self.lookahead {
                Some((_, keyword_span)) => keyword_span,
                None => Span::default(),
            };
            self.eat_token(&Token::MustUse)?;
            attributes.push(Attribute::new(
                MUST_USE_ATTRIBUTE.to_string(),
                None,
                span,
                AttributeSpelling::DeprecatedKeyword,
            ));
        }

        self.eat_token(&Token::Enum)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;

        let (variants, methods) = self.enum_body()?;

        if variants.is_empty() {
            return Err(self.error_missing_members(SyntaxErrorKind::MissingEnumMembers));
        }

        Ok(ast::enum_statement(
            name,
            generic_types,
            variants,
            methods,
            visibility,
            attributes,
        ))
    }

    /// Parses an enum body containing variant declarations and optionally method declarations.
    fn enum_body(&mut self) -> Result<(Vec<Expression>, Vec<Statement>), SyntaxError> {
        let mut variants = vec![];
        let mut methods = vec![];

        match &self.lookahead {
            Some((Token::Colon, _)) => {
                // Inline form — only variants allowed inline
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self.lookahead.is_some() {
                    variants.push(self.enum_value_expression()?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        variants.push(self.enum_value_expression()?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form — variants first, then optional method declarations
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        match &self.lookahead {
                            Some((Token::Fn, _))
                            | Some((Token::Async, _))
                            | Some((Token::Gpu, _)) => {
                                let stmt = self.function_declaration(MemberVisibility::Public)?;
                                methods.push(stmt);
                            }
                            Some((Token::Public, _)) => {
                                self.eat_token(&Token::Public)?;
                                let stmt = self.function_declaration(MemberVisibility::Public)?;
                                methods.push(stmt);
                            }
                            Some((Token::Private, _)) => {
                                self.eat_token(&Token::Private)?;
                                let stmt = self.function_declaration(MemberVisibility::Private)?;
                                methods.push(stmt);
                            }
                            _ => {
                                variants.push(self.enum_value_expression()?);
                            }
                        }
                        self.try_eat_expression_end()?;
                    }
                    self.eat_token(&Token::Dedent)?;
                }
            }
            _ => {
                return Err(self.error_unexpected_lookahead_token(
                    "either a colon for inline enums or an indentation for block enums",
                ));
            }
        }

        Ok((variants, methods))
    }
}
