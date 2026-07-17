// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn call_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.primary_expression()?;

        loop {
            if !self.lookahead_is_member_expression_boundary() {
                break;
            }

            let should_continue = match &self.lookahead {
                Some((Token::Dot, _)) => {
                    expression = self.member_access(expression)?;
                    true
                }
                Some((Token::LBracket, _)) => {
                    expression = self.index_access(expression)?;
                    true
                }
                Some((Token::LParen, _)) => {
                    expression = self.call_access(expression)?;
                    true
                }
                Some((Token::LessThan, lookahead_span)) => {
                    if lookahead_span.start == expression.span.end {
                        expression = self.generic_arg_access(expression)?;
                        true
                    } else {
                        false
                    }
                }
                Some((Token::Float, _)) if self.is_tuple_field_access() => {
                    expression = self.tuple_field_access(expression)?;
                    true
                }
                Some((Token::As, _)) => {
                    expression = self.cast_access(expression)?;
                    true
                }
                _ => false,
            };

            if !should_continue {
                break;
            }
        }

        Ok(expression)
    }

    fn is_tuple_field_access(&self) -> bool {
        if let Some((Token::Float, span)) = &self.lookahead {
            let float_text = &self.source[span.start..span.end];
            if let Some(int_part) = float_text.strip_prefix('.') {
                int_part.chars().all(|c| c.is_ascii_digit() || c == '_')
            } else {
                false
            }
        } else {
            false
        }
    }

    fn member_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Dot)?;
        let property = if self.match_lookahead_type(|t| matches!(t, Token::Int)) {
            self.literal_expression()?
        } else {
            self.identifier()?
        };
        let span = Span::new(expression.span.start, property.span.end);
        Ok(ast::member_with_span(expression, property, span))
    }

    fn index_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBracket)?;
        let index = self.expression()?;
        let (_, rbracket_span) = self.eat_token(&Token::RBracket)?;
        let span = Span::new(expression.span.start, rbracket_span.end);
        Ok(ast::index_with_span(expression, index, span))
    }

    fn call_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        let (args, rparen_span) = self.arguments()?;
        let span = Span::new(expression.span.start, rparen_span.end);
        Ok(ast::call_with_span(expression, args, span))
    }

    /// `foo<T>` vs `a < b`: whitespace between the expression and `<` means
    /// comparison, no whitespace means a generic argument list. The caller
    /// checks the whitespace rule before dispatching here.
    fn generic_arg_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        let args = self.multiple_generic_arguments()?;
        let end = args
            .last()
            .map(|a| a.span.end)
            .unwrap_or(expression.span.end);
        let span = Span::new(expression.span.start, end);
        Ok(ast::type_declaration_expression_with_span(
            expression,
            Some(args),
            TypeDeclarationKind::None,
            None,
            span,
        ))
    }

    /// Tuple access `t.0` tokenizes as `Identifier(t)` then `Float(.0)`.
    /// Floats starting with `.` followed by an integer are rewritten as
    /// member-access with an integer property.
    /// Caller must ensure the Float token is in tuple-field format.
    fn tuple_field_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        let span = self.current_token_span();
        let float_text = &self.source[span.start..span.end];
        let Some(int_part) = float_text.strip_prefix('.') else {
            return Err(self.error_unexpected_token("tuple field access", "float literal"));
        };

        self.eat_token(&Token::Float)?;

        let val_str = int_part.replace("_", "");
        let val = val_str
            .parse::<i128>()
            .map_err(|_| self.error_unexpected_token("valid integer", "number too large"))?;

        let prop_span = Span::new(span.start + 1, span.end);
        let property = ast::literal_with_span(ast::int_literal(val), prop_span);

        let total_span = Span::new(expression.span.start, property.span.end);
        Ok(ast::member_with_span(expression, property, total_span))
    }

    fn cast_access(&mut self, expression: Expression) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::As)?;
        let target_type = self
            .type_expression()?
            .ok_or_else(|| self.error_unexpected_token("type", "after 'as'"))?;
        let span = Span::new(expression.span.start, target_type.span.end);
        Ok(ast::cast_with_span(expression, target_type, span))
    }

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
