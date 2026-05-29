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
use crate::ast::statement::BindingResidency;
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

        if let Some(error) = self.detect_residency_mismatch(left, op, right, context) {
            self.report_error(error, span);
            return ast_factory::make_type(TypeKind::Error);
        }

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
        let lhs_type = self.infer_assignment_target(lhs, span, context);

        self.check_division_by_zero_assignment(op, rhs);

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

    fn infer_assignment_target(
        &mut self,
        lhs: &LeftHandSideExpression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        match lhs {
            LeftHandSideExpression::Identifier(id_expr) => {
                self.infer_assignment_to_identifier(id_expr, span, context)
            }
            LeftHandSideExpression::Member(member_expr) => {
                self.infer_assignment_to_member(member_expr, span, context)
            }
            LeftHandSideExpression::Index(index_expr) => {
                self.infer_assignment_to_index(index_expr, span, context)
            }
        }
    }

    fn infer_assignment_to_identifier(
        &mut self,
        id_expr: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let ExpressionKind::Identifier(name, _) = &id_expr.node else {
            self.report_error("Invalid assignment target".to_string(), span);
            return ast_factory::make_type(TypeKind::Error);
        };
        if name != "self" && context.resolve_info(name).is_some() && !context.is_mutable(name) {
            let msg = if context.is_constant(name) {
                format!("Cannot assign to constant '{}'", name)
            } else {
                format!("Cannot assign to immutable variable '{}'", name)
            };
            self.report_error(msg, span);
        }
        self.infer_identifier(name, id_expr.span, context)
    }

    fn infer_assignment_to_member(
        &mut self,
        member_expr: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let ExpressionKind::Member(obj, prop) = &member_expr.node else {
            return ast_factory::make_type(TypeKind::Error);
        };
        if !self.is_mutable_expression(obj, context) {
            self.report_error(
                "Cannot assign to field of immutable variable".to_string(),
                span,
            );
        }
        self.infer_member(obj, prop, member_expr.span, context)
    }

    fn infer_assignment_to_index(
        &mut self,
        index_expr: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let ExpressionKind::Index(obj, index) = &index_expr.node else {
            return ast_factory::make_type(TypeKind::Error);
        };
        if !self.is_mutable_expression(obj, context) {
            self.report_error(
                "Cannot assign to element of immutable variable".to_string(),
                span,
            );
        }
        self.infer_index(obj, index, index_expr.span, context)
    }

    fn check_division_by_zero_assignment(&mut self, op: &AssignmentOp, rhs: &Expression) {
        if !matches!(op, AssignmentOp::AssignDiv | AssignmentOp::AssignMod) {
            return;
        }
        if let ExpressionKind::Literal(lit) = &rhs.node {
            if lit.is_zero() {
                self.report_error("Division by zero".to_string(), rhs.span);
            }
        }
    }

    /// Returns the mixed-residency diagnostic when `left` and `right`
    /// reference identifiers whose binding residencies differ. Returns
    /// `None` otherwise (including when either operand is a non-identifier
    /// expression — those carry no recorded residency yet).
    fn detect_residency_mismatch(
        &self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        context: &Context,
    ) -> Option<String> {
        let (left_name, left_residency) = identifier_residency(left, context)?;
        let (right_name, right_residency) = identifier_residency(right, context)?;
        if left_residency == right_residency {
            return None;
        }
        let action = binary_op_action(op)?;
        let (gpu_name, host_name) = match (left_residency, right_residency) {
            (BindingResidency::Gpu, BindingResidency::Host) => (left_name, right_name),
            (BindingResidency::Host, BindingResidency::Gpu) => (right_name, left_name),
            // Two equal residencies were filtered above; both arms are the only
            // remaining mixed combinations.
            (BindingResidency::Host, BindingResidency::Host)
            | (BindingResidency::Gpu, BindingResidency::Gpu) => return None,
        };
        Some(format!(
            "cannot {action} gpu-resident '{gpu_name}' and host-resident '{host_name}'; \
             bring both to the same residency first."
        ))
    }
}

/// Returns `(name, residency)` when `expr` is a bare identifier reference
/// to a known symbol. Returns `None` for compound expressions, unresolved
/// names, or qualified `Type::id` paths.
fn identifier_residency<'a>(
    expr: &'a Expression,
    context: &Context,
) -> Option<(&'a str, BindingResidency)> {
    let ExpressionKind::Identifier(name, None) = &expr.node else {
        return None;
    };
    let info = context.resolve_info(name)?;
    Some((name.as_str(), info.residency))
}

/// Maps a binary operator to the verb used in the mixed-residency
/// diagnostic. Returns `None` for operators where mixed-residency operands
/// have no meaningful verb (currently only the arithmetic operators are
/// diagnosed).
fn binary_op_action(op: &BinaryOp) -> Option<&'static str> {
    match op {
        BinaryOp::Add => Some("add"),
        BinaryOp::Sub => Some("subtract"),
        BinaryOp::Mul => Some("multiply"),
        BinaryOp::Div => Some("divide"),
        BinaryOp::Mod => Some("take the remainder of"),
        BinaryOp::BitwiseAnd
        | BinaryOp::BitwiseOr
        | BinaryOp::BitwiseXor
        | BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::LessThan
        | BinaryOp::LessThanEqual
        | BinaryOp::GreaterThan
        | BinaryOp::GreaterThanEqual
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::In
        | BinaryOp::NullCoalesce
        | BinaryOp::Not
        | BinaryOp::Range => None,
    }
}
