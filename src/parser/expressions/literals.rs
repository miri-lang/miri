// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn list_literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBracket)?;

        let mut elements = vec![];
        while self.match_lookahead_type(|t| t != &Token::RBracket) {
            elements.push(self.expression()?);
            if !self.lookahead_is_comma() {
                break;
            }
            self.eat_token(&Token::Comma)?;
        }

        self.eat_token(&Token::RBracket)?;
        Ok(ast::list(elements))
    }

    /*
     */
    pub(crate) fn brace_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBrace)?;

        // If the next token is a closing brace, it's an empty map.
        if self.match_lookahead_type(|t| t == &Token::RBrace) {
            self.eat_token(&Token::RBrace)?;
            return Ok(ast::map(vec![]));
        }

        // Parse the first expression.
        let first_expr = self.expression()?;

        // Look ahead for a colon to distinguish between a map and a set.
        if self.lookahead_is_colon() {
            // It's a map.
            self.eat_token(&Token::Colon)?;
            let first_value = self.expression()?;
            let mut pairs = vec![(first_expr, first_value)];

            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                let key = self.expression()?;
                self.eat_token(&Token::Colon)?;
                let value = self.expression()?;
                pairs.push((key, value));
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::map(pairs))
        } else {
            // It's a set.
            let mut elements = vec![first_expr];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                elements.push(self.expression()?);
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::set(elements))
        }
    }

    /*
     */
    pub(crate) fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let span = if let Some((_, span)) = &self._lookahead {
            *span
        } else {
            return Err(self.error_eof());
        };
        let literal = self.literal()?;
        Ok(ast::literal_with_span(literal, span))
    }
}
