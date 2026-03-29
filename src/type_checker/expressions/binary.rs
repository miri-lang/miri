// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression type inference for the type checker.
//!
//! This module implements type inference for all expression kinds in Miri.
//! The main entry point is [`TypeChecker::infer_expression`], which dispatches
//! to specialized inference methods based on the expression kind.
//!
//! # Supported Expressions
//!
//! ## Literals
//! - Integer, float, string, boolean, and none literals
//!
//! ## Operators
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`<`, `>`, `==`, etc.)
//! - Logical: `and`, `or`
//! - Unary: `-`, `+`, `not`, `~`, `await`
//!
//! ## Collections
//! - Lists: `[1, 2, 3]` → `List<int>`
//! - Maps: `{"a": 1}` → `Map<string, int>`
//! - Sets: `{1, 2, 3}` → `Set<int>`
//! - Tuples: `(1, "a")` → `(int, string)`
//! - Ranges: `1..10` → `Range<int>`
//!
//! ## Access
//! - Member access: `obj.field`
//! - Index access: `list[0]`, `map["key"]`
//!
//! ## Functions
//! - Function calls with generic type inference
//! - Lambda expressions with type inference
//! - Method calls on objects
//!
//! ## Control Flow
//! - Conditional expressions: `x if cond else y`
//! - Match expressions with pattern matching
//!
//! ## Types
//! - Struct instantiation: `Point { x: 1, y: 2 }`
//! - Enum variant construction: `Ok(value)`, `Err(error)`
//! - Generic type instantiation

use crate::ast::factory as ast_factory;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Infers the type of a binary operation.
    ///
    /// Checks compatibility of operands and determines the result type.
    pub(crate) fn infer_binary(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let left_ty = self.infer_expression(left, context);
        let right_ty = self.infer_expression(right, context);

        if matches!(op, BinaryOp::Div | BinaryOp::Mod) {
            let is_zero = match &right.node {
                ExpressionKind::Literal(lit) => lit.is_zero(),
                ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                    matches!(&operand.node, ExpressionKind::Literal(lit) if lit.is_zero())
                }
                _ => false,
            };
            if is_zero {
                self.report_error("Division by zero".to_string(), right.span);
                return ast_factory::make_type(TypeKind::Error);
            }
        }

        // Suppress cascade: if either operand already has an error type, propagate silently
        if matches!(left_ty.kind, TypeKind::Error) || matches!(right_ty.kind, TypeKind::Error) {
            return ast_factory::make_type(TypeKind::Error);
        }

        match self.check_binary_op_types(&left_ty, op, &right_ty, context) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                ast_factory::make_type(TypeKind::Error)
            }
        }
    }

    pub(crate) fn infer_logical(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        self.infer_binary(left, op, right, span, context)
    }

    pub(crate) fn infer_assignment(
        &mut self,
        lhs: &LeftHandSideExpression,
        op: &AssignmentOp,
        rhs: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let rhs_type = self.infer_expression(rhs, context);
        let lhs_type = match lhs {
            LeftHandSideExpression::Identifier(id_expr) => {
                if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                    // 'self' is always mutable in class context (for constructor assignment)
                    // Only check mutability if the variable is actually defined;
                    // undefined variables will be reported by infer_identifier below.
                    if name != "self"
                        && context.resolve_info(name).is_some()
                        && !context.is_mutable(name)
                    {
                        if context.is_constant(name) {
                            self.report_error(
                                format!("Cannot assign to constant '{}'", name),
                                span,
                            );
                        } else {
                            self.report_error(
                                format!("Cannot assign to immutable variable '{}'", name),
                                span,
                            );
                        }
                    }
                    self.infer_identifier(name, id_expr.span, context)
                } else {
                    self.report_error("Invalid assignment target".to_string(), span);
                    ast_factory::make_type(TypeKind::Error)
                }
            }
            LeftHandSideExpression::Member(member_expr) => {
                if let ExpressionKind::Member(obj, prop) = &member_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to field of immutable variable".to_string(),
                            span,
                        );
                    }
                    self.infer_member(obj, prop, member_expr.span, context)
                } else {
                    ast_factory::make_type(TypeKind::Error)
                }
            }
            LeftHandSideExpression::Index(index_expr) => {
                if let ExpressionKind::Index(obj, index) = &index_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to element of immutable variable".to_string(),
                            span,
                        );
                    }
                    self.infer_index(obj, index, index_expr.span, context)
                } else {
                    ast_factory::make_type(TypeKind::Error)
                }
            }
        };

        if matches!(op, AssignmentOp::AssignDiv | AssignmentOp::AssignMod) {
            if let ExpressionKind::Literal(lit) = &rhs.node {
                if lit.is_zero() {
                    self.report_error("Division by zero".to_string(), rhs.span);
                }
            }
        }

        if !self.are_compatible(&lhs_type, &rhs_type, context) {
            self.report_error(
                format!(
                    "Type mismatch in assignment: cannot assign {} to {}",
                    rhs_type, lhs_type
                ),
                span,
            );
        }

        lhs_type
    }
}
