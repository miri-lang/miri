// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::utils::{
    is_additive_op, is_equality_op, is_logical_and_op, is_logical_or_op, is_multiplicative_op,
    is_null_coalesce_op, is_relational_op,
};
use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn relational_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::range_expression,
            is_relational_op,
            Self::eat_relational_op,
            ast::binary_with_span,
        )
    }

    /*
     */
    pub(crate) fn equality_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::relational_expression,
            is_equality_op,
            Self::eat_equality_op,
            ast::binary_with_span,
        )
    }

    /*
     */
    pub(crate) fn logical_and_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::equality_expression,
            is_logical_and_op,
            Self::eat_logical_and_op,
            ast::logical_with_span,
        )
    }

    /*
     */
    pub(crate) fn logical_or_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op,
            ast::logical_with_span,
        )
    }

    /*
     */
    pub(crate) fn null_coalesce_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::logical_or_expression,
            is_null_coalesce_op,
            Self::eat_null_coalesce_op,
            ast::logical_with_span,
        )
    }

    /*
     */
    pub(crate) fn additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op,
            ast::binary_with_span,
        )
    }

    /*
     */
    pub(crate) fn multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression_precedence(
            Self::unary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op,
            ast::binary_with_span,
        )
    }

    /// Generic left-associative binary expression parser used by all
    /// precedence levels. Parses `operand (op operand)*` using the provided
    /// `create_branch` to parse each operand, `op_predicate` and `eat_op`
    /// to match and consume operators, and `create_expression` to build the
    /// resulting AST node.
    pub(crate) fn binary_expression_precedence<F, G, E>(
        &mut self,
        mut create_branch: F,
        op_predicate: fn(&Token) -> bool,
        mut eat_op: G,
        mut create_expression: E,
    ) -> Result<Expression, SyntaxError>
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
        E: FnMut(Expression, BinaryOp, Expression, Span) -> Expression,
    {
        let mut left = create_branch(self)?;

        while self.match_lookahead_type(op_predicate) {
            let op = match eat_op(self) {
                Ok(value) => value,
                Err(value) => return value,
            };

            let right = create_branch(self)?;

            let span = Span::new(left.span.start, right.span.end);
            left = create_expression(left, op, right, span);
        }

        Ok(left)
    }
}
