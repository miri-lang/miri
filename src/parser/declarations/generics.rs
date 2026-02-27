// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::utils::is_inheritance_modifier;
use super::super::Parser;

impl<'source> Parser<'source> {
    /*
    */
    pub(crate) fn generic_types_declaration(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LessThan)?;

        let mut types = vec![self.generic_type()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            types.push(self.generic_type()?);
        }

        self.eat_token(&Token::GreaterThan)?;
        Ok(types)
    }

    /*
    */
    pub(crate) fn generic_type(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        if self._lookahead.is_none() || !self.lookahead_is_inheritance_modifier() {
            return Ok(ast::generic_type_expression(
                identifier,
                None,
                TypeDeclarationKind::None,
            ));
        }

        let token_span = self.eat(is_inheritance_modifier, || {
            "extends, includes or implements".to_string()
        })?;
        let kind = match token_span.0 {
            Token::Extends => TypeDeclarationKind::Extends,
            Token::Implements => TypeDeclarationKind::Implements,
            Token::Includes => TypeDeclarationKind::Includes,
            _ => TypeDeclarationKind::None,
        };

        let typ = self.type_expression()?.map(Box::new);

        Ok(ast::generic_type_expression(identifier, typ, kind))
    }

}
