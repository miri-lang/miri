// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn pattern(&mut self) -> Result<Pattern, SyntaxError> {
        match &self._lookahead {
            Some((Token::Default, _)) => {
                self.eat_token(&Token::Default)?;
                Ok(Pattern::Default)
            }
            Some((Token::Identifier, _)) => {
                let name = self.parse_simple_identifier()?;
                let mut pattern = Pattern::Identifier(name);

                while self.match_lookahead_type(|t| t == &Token::Dot) {
                    self.eat_token(&Token::Dot)?;
                    let member = self.parse_simple_identifier()?;
                    pattern = Pattern::Member(Box::new(pattern), member);
                }

                // Check for enum variant with bindings: Color.Red(x, y)
                if self.match_lookahead_type(|t| t == &Token::LParen) {
                    if let Pattern::Tuple(bindings) = self.tuple_pattern()? {
                        pattern = Pattern::EnumVariant(Box::new(pattern), bindings);
                    }
                }

                Ok(pattern)
            }
            Some((Token::LParen, _)) => self.tuple_pattern(),
            Some((Token::Regex(_), _)) => {
                if let Literal::Regex(regex_token) = self.regex_literal()? {
                    Ok(Pattern::Regex(regex_token))
                } else {
                    Err(self.error_unexpected_lookahead_token("regex pattern"))
                }
            }
            _ if self.lookahead_is_literal() => {
                let literal = self.literal()?;
                Ok(Pattern::Literal(literal))
            }
            _ => Err(self
                .error_unexpected_lookahead_token("a pattern (literal, identifier, or default)")),
        }
    }

    /*
     */
    pub(crate) fn tuple_pattern(&mut self) -> Result<Pattern, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let mut patterns = Vec::new();
        if !self.lookahead_is_rparen() {
            patterns.push(self.pattern()?);
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.lookahead_is_rparen() {
                    break;
                } // Allow trailing comma
                patterns.push(self.pattern()?);
            }
        }
        self.eat_token(&Token::RParen)?;
        Ok(Pattern::Tuple(patterns))
    }
}
