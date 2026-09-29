// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn list_literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let open = self.eat_token(&Token::LBracket)?;

        let mut elements = vec![];
        while self.match_lookahead_type(|t| t != &Token::RBracket) {
            elements.push(self.expression()?);
            if !self.lookahead_is_comma() {
                break;
            }
            self.eat_token(&Token::Comma)?;
        }

        let close = self.eat_token(&Token::RBracket)?;
        let size = ast::int_literal_expression(elements.len() as i128);
        let mut literal = ast::array(elements, Box::new(size));
        literal.span = Span::new(open.1.start, close.1.end);
        Ok(literal)
    }

    /// A map or set literal, spanning its braces.
    pub(crate) fn brace_expression(&mut self) -> Result<Expression, SyntaxError> {
        let open = self.eat_token(&Token::LBrace)?;
        let (mut literal, end) = self.brace_literal()?;
        literal.span = Span::new(open.1.start, end);
        Ok(literal)
    }

    /// The map or set after its opening brace, with where its closing brace
    /// ends.
    fn brace_literal(&mut self) -> Result<(Expression, usize), SyntaxError> {
        // If the next token is a closing brace, it's an empty map.
        if self.match_lookahead_type(|t| t == &Token::RBrace) {
            let close = self.eat_token(&Token::RBrace)?;
            return Ok((ast::map(vec![]), close.1.end));
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
            let close = self.eat_token(&Token::RBrace)?;
            Ok((ast::map(pairs), close.1.end))
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
            let close = self.eat_token(&Token::RBrace)?;
            Ok((ast::set(elements), close.1.end))
        }
    }

    pub(crate) fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let span = if let Some((_, span)) = &self.lookahead {
            *span
        } else {
            return Err(self.error_eof());
        };
        let literal = self.literal()?;
        Ok(ast::literal_with_span(literal, span))
    }
}
