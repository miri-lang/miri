// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
    */
    pub(crate) fn call_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.primary_expression()?;

        loop {
            if !self.lookahead_is_member_expression_boundary() {
                break;
            }

            expression = match &self._lookahead {
                Some((Token::Dot, _)) => {
                    self.eat_token(&Token::Dot)?;
                    let property = if self.match_lookahead_type(|t| matches!(t, Token::Int)) {
                        self.literal_expression()?
                    } else {
                        self.identifier()?
                    };
                    let span = Span::new(expression.span.start, property.span.end);
                    ast::member_with_span(expression, property, span)
                }
                Some((Token::LBracket, _)) => {
                    self.eat_token(&Token::LBracket)?;
                    let index = self.expression()?;
                    let (_, rbracket_span) = self.eat_token(&Token::RBracket)?;
                    let span = Span::new(expression.span.start, rbracket_span.end);
                    ast::index_with_span(expression, index, span)
                }
                Some((Token::LParen, _)) => {
                    let (args, rparen_span) = self.arguments()?;
                    let span = Span::new(expression.span.start, rparen_span.end);
                    ast::call_with_span(expression, args, span)
                }
                Some((Token::LessThan, _)) => {
                    // Heuristic: If there is whitespace between the expression and '<', it's a comparison.
                    // If there is no whitespace, it's a generic argument list.
                    // e.g. `a < b` (comparison), `foo<T>` (generic)
                    let prev_end = expression.span.end;
                    let current_start = self._lookahead.as_ref().unwrap().1.start;

                    if current_start > prev_end {
                        break;
                    }

                    let args = self.multiple_element_type_expressions(
                        "Generic arguments",
                        &Token::LessThan,
                        &Token::GreaterThan,
                    )?;
                    let end = args
                        .last()
                        .map(|a| a.span.end)
                        .unwrap_or(expression.span.end);
                    let span = Span::new(expression.span.start, end);
                    IdNode::new(
                        0,
                        ExpressionKind::TypeDeclaration(
                            Box::new(expression),
                            Some(args),
                            TypeDeclarationKind::None,
                            None,
                        ),
                        span,
                    )
                }
                Some((Token::Float, _)) => {
                    let span = self.current_token_span();
                    let source = self.source;
                    let float_text = &source[span.start..span.end];

                    if let Some(int_part) = float_text.strip_prefix('.') {
                        // This might be a tuple access like `t.0` which tokenizes as Identifier `t` then Float `.0`.
                        // We need to treat `.0` as a dot followed by an integer.

                        // Verify the rest is a valid integer (only digits and underscores).
                        if int_part.chars().all(|c| c.is_ascii_digit() || c == '_') {
                            // Valid tuple access pattern.
                            self.eat_token(&Token::Float)?;

                            // Parse the integer value.
                            let val_str = int_part.replace("_", "");
                            // We use i128 to be safe, though tuple indices are usually small.
                            let val = val_str.parse::<i128>().map_err(|_| {
                                self.error_unexpected_token("valid integer", "number too large")
                            })?;

                            // Create the property node.
                            // The span of the property is the float span sans the leading dot.
                            let prop_span = Span::new(span.start + 1, span.end);
                            let property = ast::literal_with_span(ast::int_literal(val), prop_span);

                            let span = Span::new(expression.span.start, property.span.end);
                            ast::member_with_span(expression, property, span)
                        } else {
                            // Contains exponent or other float chars, treat as boundary stop (not member access).
                            break;
                        }
                    } else {
                        // Float doesn't start with dot (e.g. `1.0`), not a member access here.
                        break;
                    }
                }
                _ => break,
            };
        }

        Ok(expression)
    }

    /*
    */
    pub(crate) fn arguments(&mut self) -> Result<(Vec<Expression>, Span), SyntaxError> {
        self.eat_token(&Token::LParen)?;

        let argument_list = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.argument_list()?
        };

        let (_, span) = self.eat_token(&Token::RParen)?;
        Ok((argument_list, span))
    }

    /*
    */
    pub(crate) fn argument_list(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        let mut args = Vec::new();

        loop {
            if self.lookahead_is_rparen() {
                break;
            }

            let expr = self.assignment_expression()?;

            if self.match_lookahead_type(|t| t == &Token::Colon) {
                if let ExpressionKind::Identifier(name, None) = expr.node {
                    self.eat_token(&Token::Colon)?;
                    let value = self.assignment_expression()?;
                    let span = Span::new(expr.span.start, value.span.end);
                    args.push(ast::named_argument_with_span(name, value, span));
                } else {
                    return Err(self.error_unexpected_token("identifier for named argument", ":"));
                }
            } else {
                args.push(expr);
            }

            if self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
            } else {
                break;
            }
        }

        Ok(args)
    }

}
