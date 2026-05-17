// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::expression::{index, member};
use super::primitives::identifier;
use crate::ast::expression::{Expression, LeftHandSideExpression};

/// Wraps an expression as a left-hand side identifier.
pub fn lhs_identifier_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Identifier(Box::new(expr))
}

/// Creates a left-hand side identifier from a string name.
pub fn lhs_identifier(name: &str) -> LeftHandSideExpression {
    lhs_identifier_from_expr(identifier(name))
}

/// Wraps an expression as a left-hand side member access.
pub fn lhs_member_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Member(Box::new(expr))
}

/// Creates a left-hand side member access.
pub fn lhs_member(object: Expression, property: Expression) -> LeftHandSideExpression {
    lhs_member_from_expr(member(object, property))
}

/// Wraps an expression as a left-hand side index access.
pub fn lhs_index_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Index(Box::new(expr))
}

/// Creates a left-hand side index access.
pub fn lhs_index(object: Expression, idx: Expression) -> LeftHandSideExpression {
    lhs_index_from_expr(index(object, idx))
}
