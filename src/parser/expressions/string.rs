// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::literals::unescape_string;
use super::super::Parser;

impl<'source> Parser<'source> {
    /*
    */
    pub(crate) fn formatted_string_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut parts = Vec::new();

        // Consume the opening formatted-string token and extract its text.
        let (start_token, _) = self.eat(
            |t| matches!(t, Token::FormattedStringStart(_)),
            || "formatted string start".to_string(),
        )?;
        if let Token::FormattedStringStart(start_text) = start_token {
            if !start_text.is_empty() {
                let unescaped = unescape_string(&start_text);
                parts.push(ast::literal(ast::string_literal(&unescaped)));
            }
        }

        // Check for immediate end (f-string with no expressions).
        if matches!(&self._lookahead, Some((Token::FormattedStringEnd(_), _))) {
            let (end_token, _) = self.eat(
                |t| matches!(t, Token::FormattedStringEnd(_)),
                || "formatted string end".to_string(),
            )?;
            if let Token::FormattedStringEnd(end_text) = end_token {
                if !end_text.is_empty() {
                    let unescaped = unescape_string(&end_text);
                    parts.push(ast::literal(ast::string_literal(&unescaped)));
                }
            }
            return Ok(ast::f_string(parts));
        }

        while self._lookahead.is_some() {
            parts.push(self.expression()?);

            if matches!(&self._lookahead, Some((Token::FormattedStringMiddle(_), _))) {
                let (mid_token, _) = self.eat(
                    |t| matches!(t, Token::FormattedStringMiddle(_)),
                    || "formatted string middle".to_string(),
                )?;
                if let Token::FormattedStringMiddle(middle_text) = mid_token {
                    if !middle_text.is_empty() {
                        let unescaped = unescape_string(&middle_text);
                        parts.push(ast::literal(ast::string_literal(&unescaped)));
                    }
                }
            } else if matches!(&self._lookahead, Some((Token::FormattedStringEnd(_), _))) {
                let (end_token, _) = self.eat(
                    |t| matches!(t, Token::FormattedStringEnd(_)),
                    || "formatted string end".to_string(),
                )?;
                if let Token::FormattedStringEnd(end_text) = end_token {
                    if !end_text.is_empty() {
                        let unescaped = unescape_string(&end_text);
                        parts.push(ast::literal(ast::string_literal(&unescaped)));
                    }
                }
                break;
            } else {
                return Err(
                    self.error_unexpected_lookahead_token("middle or end of a formatted string")
                );
            }
        }

        Ok(ast::f_string(parts))
    }

}
