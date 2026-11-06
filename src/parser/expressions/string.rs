// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::format_spec::FormatSpec;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::literals::unescape_string;
use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn formatted_string_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut parts = Vec::new();

        // Consume the opening formatted-string token and extract its text.
        let (start_token, start_span) = self.eat(
            |t| matches!(t, Token::FormattedStringStart(_)),
            || "formatted string start".to_string(),
        )?;
        if let Token::FormattedStringStart(start_text) = start_token {
            push_text_part(&mut parts, &start_text);
        }

        // An f-string with no expressions ends right after its opening text.
        if matches!(&self.lookahead, Some((Token::FormattedStringEnd(_), _))) {
            let end = self.formatted_string_end(&mut parts)?;
            let span = Span::new(start_span.start, end);
            return Ok(ast::f_string_with_span(parts, Vec::new(), span));
        }

        let mut specs = Vec::new();
        let mut end = start_span.end;
        while self.lookahead.is_some() {
            parts.push(self.expression()?);
            if let Some((Token::FormatSpec(_), _)) = &self.lookahead {
                let (spec_token, span) = self.eat(
                    |t| matches!(t, Token::FormatSpec(_)),
                    || "format spec".to_string(),
                )?;
                if let Token::FormatSpec(text) = spec_token {
                    specs.push((parts.len() - 1, FormatSpec { text: *text, span }));
                }
            }

            if matches!(&self.lookahead, Some((Token::FormattedStringMiddle(_), _))) {
                let (mid_token, _) = self.eat(
                    |t| matches!(t, Token::FormattedStringMiddle(_)),
                    || "formatted string middle".to_string(),
                )?;
                if let Token::FormattedStringMiddle(middle_text) = mid_token {
                    push_text_part(&mut parts, &middle_text);
                }
            } else if matches!(&self.lookahead, Some((Token::FormattedStringEnd(_), _))) {
                end = self.formatted_string_end(&mut parts)?;
                break;
            } else {
                return Err(
                    self.error_unexpected_lookahead_token("middle or end of a formatted string")
                );
            }
        }

        Ok(ast::f_string_with_span(
            parts,
            specs,
            Span::new(start_span.start, end),
        ))
    }

    /// Consumes the token closing an f-string, adds its trailing text to
    /// `parts`, and returns the offset just past the closing quote.
    fn formatted_string_end(&mut self, parts: &mut Vec<Expression>) -> Result<usize, SyntaxError> {
        let (end_token, end_span) = self.eat(
            |t| matches!(t, Token::FormattedStringEnd(_)),
            || "formatted string end".to_string(),
        )?;
        if let Token::FormattedStringEnd(end_text) = end_token {
            push_text_part(parts, &end_text);
        }
        Ok(end_span.end)
    }
}

/// Adds the literal text between an f-string's interpolations as a part, if
/// there is any.
fn push_text_part(parts: &mut Vec<Expression>, text: &str) {
    if !text.is_empty() {
        let unescaped = unescape_string(text);
        parts.push(ast::literal(ast::string_literal(&unescaped)));
    }
}
