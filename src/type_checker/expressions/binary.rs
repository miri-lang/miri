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
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

/// True for the numeric arithmetic operators (`+`, `-`, `*`, `/`, `%`) — the
/// operators over which an `f16` operand narrows a bare float literal.
fn is_arithmetic_op(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod
    )
}

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
            self.report_error(DiagnosticCode::TarGpuResidencyViolation, error, span);
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
                self.report_error(
                    DiagnosticCode::TypConstEvalArithmetic,
                    "Division by zero".to_string(),
                    right.span,
                );
                return ast_factory::make_type(TypeKind::Error);
            }
        }

        // Suppress cascade: if either operand already has an error type, propagate silently
        if matches!(left_ty.kind, TypeKind::Error) || matches!(right_ty.kind, TypeKind::Error) {
            return ast_factory::make_type(TypeKind::Error);
        }

        // A bare float literal defaults to `f32`/`f64`, so `f16_elem * 2.0` would
        // otherwise fail as a scalar-width mismatch. Narrow the literal operand to
        // `f16` (retyping it so MIR/WGSL render the `2.0h` suffix) and keep the
        // result `f16`. Narrowing is literal-only — a genuine `f32` value stays a
        // mismatch, since silently narrowing a runtime value would lose precision.
        if is_arithmetic_op(op) {
            if let Some(narrowed) = self.narrow_f16_float_literal(&left_ty, left, &right_ty, right)
            {
                return narrowed;
            }
        }

        match self.check_binary_op_types(&left_ty, op, &right_ty, context) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(DiagnosticCode::TypTypeMismatch, msg, span);
                ast_factory::make_type(TypeKind::Error)
            }
        }
    }

    /// Narrows a bare float literal operand to `f16` when the other operand is
    /// `f16`, returning the `f16` result type. Only one operand may be `f16` for
    /// narrowing to apply — two `f16` operands need no narrowing, and a genuine
    /// non-`f16` float value (a buffer element, a binding) is left to the normal
    /// compatibility check, which rejects the scalar-width mismatch.
    fn narrow_f16_float_literal(
        &mut self,
        left_ty: &Type,
        left: &Expression,
        right_ty: &Type,
        right: &Expression,
    ) -> Option<Type> {
        let left_f16 = matches!(left_ty.kind, TypeKind::F16);
        let right_f16 = matches!(right_ty.kind, TypeKind::F16);
        if left_f16 && !right_f16 && self.retype_float_literal_to_f16(right) {
            return Some(ast_factory::make_type(TypeKind::F16));
        }
        if right_f16 && !left_f16 && self.retype_float_literal_to_f16(left) {
            return Some(ast_factory::make_type(TypeKind::F16));
        }
        None
    }

    /// Records `expr` as `f16` when it is a bare float literal, optionally wrapped
    /// in a unary `+`/`-`. Retyping the recorded expression type is what makes MIR
    /// lowering emit an `f16` constant (and thus the WGSL `h` suffix). Returns
    /// `true` when `expr` was a float literal and was retyped.
    fn retype_float_literal_to_f16(&mut self, expr: &Expression) -> bool {
        match &expr.node {
            ExpressionKind::Literal(Literal::Float(_)) => {
                self.type_table
                    .types
                    .insert(expr.id, Type::new(TypeKind::F16, expr.span));
                true
            }
            ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand)
                if self.retype_float_literal_to_f16(operand) =>
            {
                self.type_table
                    .types
                    .insert(expr.id, Type::new(TypeKind::F16, expr.span));
                true
            }
            _ => false,
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
                DiagnosticCode::TypImmutabilityViolation,
                format!(
                    "Type mismatch in assignment: cannot assign {} to {}",
                    rhs_type, lhs_type
                ),
                span,
            );
        }

        if matches!(op, AssignmentOp::Assign) {
            self.check_gpu_reassignment_i32_range(lhs, rhs, &lhs_type, context);
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
            self.report_error(
                DiagnosticCode::TypImmutabilityViolation,
                "Invalid assignment target".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        };
        if name != "self" && context.resolve_info(name).is_some() && !context.is_mutable(name) {
            let msg = if context.is_constant(name) {
                format!("Cannot assign to constant '{}'", name)
            } else {
                format!("Cannot assign to immutable variable '{}'", name)
            };
            self.report_error(DiagnosticCode::TypImmutabilityViolation, msg, span);
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
                DiagnosticCode::TypImmutabilityViolation,
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
                DiagnosticCode::TypImmutabilityViolation,
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
                self.report_error(
                    DiagnosticCode::TypConstEvalArithmetic,
                    "Division by zero".to_string(),
                    rhs.span,
                );
            }
        }
    }

    /// Validates that a gpu-resident identifier's reassignment has array-literal
    /// elements within i32 range. Called only for plain (non-compound) assignments.
    fn check_gpu_reassignment_i32_range(
        &mut self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        lhs_type: &Type,
        context: &mut Context,
    ) {
        if let LeftHandSideExpression::Identifier(id_expr) = lhs {
            if self.gpu_resident_identifier(id_expr, context).is_some() {
                let elem_expr = match &lhs_type.kind {
                    TypeKind::Array(elem_expr, _) => elem_expr.as_ref(),
                    TypeKind::Custom(name, Some(args)) => {
                        use crate::ast::types::BuiltinCollectionKind;
                        if BuiltinCollectionKind::from_name(name)
                            != Some(BuiltinCollectionKind::Array)
                        {
                            return;
                        }
                        if args.is_empty() {
                            return;
                        }
                        &args[0]
                    }
                    _ => return,
                };
                self.check_gpu_i32_range_array_expr(rhs, elem_expr, context);
            }
        }
    }

    /// Returns the mixed-residency diagnostic when exactly one operand of an
    /// arithmetic expression is a gpu-resident identifier and the other is not.
    ///
    /// A gpu-resident scalar (e.g. a `gpu let` reduce result) lives in a device
    /// buffer; combining it with a host value — a literal, a host binding, or
    /// any non-gpu-resident expression — requires an explicit readback first.
    /// When both sides are gpu-resident, or neither is, the operation is
    /// well-formed and `None` is returned.
    fn detect_residency_mismatch(
        &self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        context: &Context,
    ) -> Option<String> {
        let action = binary_op_action(op)?;
        let left_gpu = gpu_resident_identifier(left, context);
        let right_gpu = gpu_resident_identifier(right, context);
        // Both gpu-resident or neither: no mismatch to diagnose.
        if left_gpu.is_some() == right_gpu.is_some() {
            return None;
        }
        let (gpu_name, host_operand) = match (left_gpu, right_gpu) {
            (Some(name), None) => (name, right),
            (None, Some(name)) => (name, left),
            // Filtered above: exactly one side is gpu-resident here.
            _ => return None,
        };
        let host_desc = match &host_operand.node {
            ExpressionKind::Identifier(name, None) => format!("host-resident '{name}'"),
            _ => "a host value".to_string(),
        };
        Some(format!(
            "cannot {action} gpu-resident '{gpu_name}' and {host_desc}; \
             bring both to the same residency first."
        ))
    }
}

/// Returns the identifier name when `expr` is a bare reference to a
/// gpu-resident binding (`gpu let` / `gpu var`), and `None` otherwise.
fn gpu_resident_identifier<'a>(expr: &'a Expression, context: &Context) -> Option<&'a str> {
    let (name, residency) = identifier_residency(expr, context)?;
    match residency {
        BindingResidency::Gpu => Some(name),
        BindingResidency::Host => None,
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
