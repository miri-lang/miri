// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn identifier(&mut self) -> Result<Expression, SyntaxError> {
        let (name, span) = if self.match_lookahead_type(|t| matches!(t, Token::None)) {
            let (_, span) = self.eat_token(&Token::None)?;
            ("None".to_string(), span)
        } else {
            if self.lookahead.is_none() {
                return Err(self.error_unexpected_lookahead_token("identifier"));
            }
            let (_, span) = self.eat_token(&Token::Identifier)?;
            (self.source[span.start..span.end].to_string(), span)
        };

        let (name, class, full_span) = match &self.lookahead {
            Some((Token::DoubleColon, _)) => {
                self.eat_token(&Token::DoubleColon)?;
                let (_, second_span) = self.eat_token(&Token::Identifier)?;

                (
                    self.source[second_span.start..second_span.end].to_string(),
                    Some(name),
                    Span::new(span.start, second_span.end),
                )
            }
            _ => (name, None, span),
        };
        Ok(ast::identifier_with_class_and_span(&name, class, full_span))
    }

    pub(crate) fn simple_identifier(&mut self) -> Result<String, SyntaxError> {
        let identifier_expr = self.identifier()?;
        if let ExpressionKind::Identifier(id, class_opt) = identifier_expr.node {
            if let Some(class) = class_opt {
                // A simple identifier cannot be namespaced.
                return Err(self
                    .error_unexpected_token("a simple identifier", &format!("{}::{}", class, id)));
            }
            Ok(id)
        } else {
            // This case should ideally not be reachable if identifier() works correctly
            Err(self.error_unexpected_token("identifier", &format!("{:?}", identifier_expr)))
        }
    }

    pub(crate) fn primary_expression(&mut self) -> Result<Expression, SyntaxError> {
        if self.lookahead.is_none() {
            return Err(self.error_eof());
        }

        if self.lookahead_is_literal() {
            return self.literal_expression();
        }

        match &self.lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            Some((Token::Identifier, _)) => self.identifier(),
            // `frame` is a contextual keyword: it names the per-frame input
            // context inside a `gpu frame` kernel body and parses as an
            // ordinary identifier in expression position. The type checker
            // binds it only within a `gpu frame` body and rejects it elsewhere.
            Some((Token::Frame, _)) => {
                let (_, span) = self.eat_token(&Token::Frame)?;
                Ok(ast::identifier_with_class_and_span("frame", None, span))
            }
            Some((Token::Super, _)) => {
                let (_, span) = self.eat_token(&Token::Super)?;
                Ok(ast::super_expression_with_span(span))
            }
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => self.lambda_expression(),
            Some((Token::LBracket, _)) => self.list_literal_expression(),
            Some((Token::LBrace, _)) => self.brace_expression(),
            Some((Token::Match, _)) => self.match_expression(),
            Some((Token::FormattedStringStart(_), _)) => self.formatted_string_expression(),
            Some((Token::If, _)) | Some((Token::Unless, _)) => self.prefix_if_expression(),
            Some((Token::Ampersand, span)) => {
                let span = *span;
                Err(SyntaxError::new(
                    SyntaxErrorKind::UnsupportedCStyleOperator {
                        found: "&&".to_string(),
                        suggestion: "and".to_string(),
                    },
                    span,
                ))
            }
            Some((Token::Pipe, span)) => {
                let span = *span;
                Err(SyntaxError::new(
                    SyntaxErrorKind::UnsupportedCStyleOperator {
                        found: "||".to_string(),
                        suggestion: "or".to_string(),
                    },
                    span,
                ))
            }
            _ => Err(self.error_unexpected_lookahead_token("an expression")),
        }
    }

    pub(crate) fn parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LParen)?;

        // Handle the empty tuple `()` case.
        if self.match_lookahead_type(|t| t == &Token::RParen) {
            self.eat_token(&Token::RParen)?;
            return Ok(ast::tuple(vec![]));
        }

        let first_expr = self.expression()?;

        // The presence of a comma is what distinguishes a tuple from a grouping parenthesis.
        if !self.lookahead_is_comma() {
            // No comma, so this is a grouping parenthesized expression.
            self.eat_token(&Token::RParen)?;
            return Ok(first_expr);
        }

        // It's a tuple. Start with the first expression we already parsed.
        let mut elements = vec![first_expr];

        // Loop through the rest of the comma-separated expressions.
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            // Handle optional trailing comma before the closing parenthesis.
            if self.match_lookahead_type(|t| t == &Token::RParen) {
                break;
            }
            elements.push(self.expression()?);
        }

        self.eat_token(&Token::RParen)?;
        Ok(ast::tuple(elements))
    }
}
