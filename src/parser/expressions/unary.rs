// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn unary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Plus, _)) => self.create_unary_expression(&Token::Plus, UnaryOp::Plus),
            Some((Token::Minus, _)) => self.create_unary_expression(&Token::Minus, UnaryOp::Negate),
            Some((Token::Not, _)) => self.create_unary_expression(&Token::Not, UnaryOp::Not),
            Some((Token::Tilde, _)) => {
                self.create_unary_expression(&Token::Tilde, UnaryOp::BitwiseNot)
            }
            Some((Token::Decrement, _)) => {
                self.create_unary_expression(&Token::Decrement, UnaryOp::Decrement)
            }
            Some((Token::Increment, _)) => {
                self.create_unary_expression(&Token::Increment, UnaryOp::Increment)
            }
            Some((Token::Await, _)) => self.create_unary_expression(&Token::Await, UnaryOp::Await),
            _ => self.left_hand_side_expression(),
        }
    }

    pub(crate) fn create_unary_expression(
        &mut self,
        token: &Token,
        op: UnaryOp,
    ) -> Result<Expression, SyntaxError> {
        let (_, span) = self.eat_token(token)?;
        let operand = self.unary_expression()?;
        let full_span = Span::new(span.start, operand.span.end);
        Ok(ast::unary_with_span(op, operand, full_span))
    }
}
