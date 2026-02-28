// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    /// Parses an enum variant declaration (name with optional associated types).
    pub fn enum_value_expression(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        let types = if self.match_lookahead_type(|t| t == &Token::LParen) {
            self.multiple_element_type_expressions(
                "Enum value type",
                &Token::LParen,
                &Token::RParen,
            )?
        } else {
            vec![]
        };

        Ok(ast::enum_value_expression(identifier, types))
    }

    /*
     */
    pub(crate) fn type_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Type)?;
        let mut declarations = vec![self.type_declaration()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            if self.lookahead_is_expression_end() {
                break; // Allow trailing comma
            }
            declarations.push(self.type_declaration()?);
        }
        self.eat_statement_end()?;
        Ok(ast::type_statement(declarations, visibility))
    }

    /*
     */
    pub(crate) fn type_declaration(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        let kind = match self._lookahead {
            Some((Token::Is, _)) => {
                self.eat_token(&Token::Is)?;
                TypeDeclarationKind::Is
            }
            Some((Token::Extends, _)) => {
                self.eat_token(&Token::Extends)?;
                TypeDeclarationKind::Extends
            }
            Some((Token::Implements, _)) => {
                self.eat_token(&Token::Implements)?;
                TypeDeclarationKind::Implements
            }
            Some((Token::Includes, _)) => {
                self.eat_token(&Token::Includes)?;
                TypeDeclarationKind::Includes
            }
            Some((Token::Comma, _)) | Some((Token::ExpressionStatementEnd, _)) => {
                // If we see a comma or the end of the statement, it means this is a continuation of a type declaration list
                return Ok(ast::type_declaration_expression(
                    name,
                    generic_types,
                    TypeDeclarationKind::None,
                    None,
                ));
            }
            _ => {
                return Err(self.error_unexpected_token(
                    "is, implements, includes or extends",
                    &self.lookahead_as_string(),
                ));
            }
        };
        let type_expr = self.type_expression()?.map(Box::new);
        Ok(ast::type_declaration_expression(
            name,
            generic_types,
            kind,
            type_expr,
        ))
    }
}
