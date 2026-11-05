// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;
use crate::parser::statements::{DeclarationBody, DeclarationMember};

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

        let opening_comment = self.opening_comment();
        let body = self.enum_body()?;

        if body.entries.is_empty() {
            return Err(self.error_missing_members(SyntaxErrorKind::MissingEnumMembers, name.span));
        }

        let mut statement = ast::enum_statement(
            name,
            generic_types,
            body.entries,
            body.methods,
            visibility,
            attributes,
        );
        statement.trivia.opening_comment = opening_comment;
        statement.trivia.member_comments = body.member_comments;
        Ok(statement)
    }

    /// Checks if the current lookahead is the contextual `static` keyword.
    fn is_contextual_static_before_fn(&self) -> bool {
        if let Some((Token::Identifier, span)) = &self.lookahead {
            &self.source[span.start..span.end] == "static"
        } else {
            false
        }
    }

    /// Parses an enum body containing variant declarations and optionally method declarations.
    fn enum_body(&mut self) -> Result<DeclarationBody, SyntaxError> {
        let mut body = DeclarationBody::default();

        match &self.lookahead {
            Some((Token::Colon, _)) => {
                // Inline form — only variants allowed inline
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self.lookahead.is_some() {
                    body.entries.push(self.enum_value_expression()?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        body.entries.push(self.enum_value_expression()?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form — variants first, then optional method declarations
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        self.declaration_member(&mut body, Self::enum_member)?;
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

        Ok(body)
    }

    /// A method or a variant of a block-form enum body.
    fn enum_member(&mut self) -> Result<DeclarationMember, SyntaxError> {
        // The contextual `static` keyword is left for `function_declaration`.
        if self.is_contextual_static_before_fn() {
            return Ok(DeclarationMember::method(
                self.function_declaration(MemberVisibility::Public)?,
            ));
        }
        match &self.lookahead {
            Some((Token::Fn, _)) | Some((Token::Async, _)) | Some((Token::Gpu, _)) => Ok(
                DeclarationMember::method(self.function_declaration(MemberVisibility::Public)?),
            ),
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                let mut method = self.function_declaration(MemberVisibility::Public)?;
                method.trivia.written_modifiers.public = true;
                Ok(DeclarationMember::method(method))
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                Ok(DeclarationMember::method(
                    self.function_declaration(MemberVisibility::Private)?,
                ))
            }
            _ => Ok(DeclarationMember::Entry(self.enum_value_expression()?)),
        }
    }
}
