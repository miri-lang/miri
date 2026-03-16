// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::utils::is_assignment_op;
use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn expression(&mut self) -> Result<Expression, SyntaxError> {
        self.depth += 1;
        if self.depth > crate::parser::MAX_PARSE_DEPTH {
            self.depth -= 1;
            let span = self
                ._lookahead
                .as_ref()
                .map(|(_, s)| *s)
                .unwrap_or(crate::error::syntax::Span::new(0, 0));
            return Err(SyntaxError::new(
                crate::error::syntax::SyntaxErrorKind::RecursionLimitExceeded,
                span,
            ));
        }
        let res = self.assignment_expression();
        self.depth -= 1;
        res
    }

    /*
     */
    pub(crate) fn assignment_expression(&mut self) -> Result<Expression, SyntaxError> {
        let left = self.conditional_expression()?;

        if !self.lookahead_is_assignment_op() {
            return Ok(left);
        }

        let op = match self.eat_binary_op(is_assignment_op) {
            Ok(token) => match token.0 {
                Token::Assign => AssignmentOp::Assign,
                Token::AssignAdd => AssignmentOp::AssignAdd,
                Token::AssignSub => AssignmentOp::AssignSub,
                Token::AssignMul => AssignmentOp::AssignMul,
                Token::AssignDiv => AssignmentOp::AssignDiv,
                Token::AssignMod => AssignmentOp::AssignMod,
                _ => return Err(self.error_unexpected_operator(token, "=, +=, -=, *=, /=, %=")),
            },
            Err(err) => return Err(err),
        };

        let left = match &left.node {
            ExpressionKind::Identifier(_, class) => {
                if class.is_some() {
                    // A left-hand side identifier cannot be namespaced.
                    return Err(self.error_invalid_left_hand_side_expression());
                }
                ast::lhs_identifier_from_expr(left)
            }
            ExpressionKind::Member(_, _) => ast::lhs_member_from_expr(left),
            ExpressionKind::Index(_, _) => ast::lhs_index_from_expr(left),
            _ => return Err(self.error_invalid_left_hand_side_expression()),
        };

        let right = self.assignment_expression()?;

        let span = Span::new(left.span().start, right.span.end);
        let assignment_expression = ast::assign_with_span(left, op, right, span);

        Ok(assignment_expression)
    }

    /*
     */
    pub(crate) fn left_hand_side_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.call_member_expression()
    }
}
