// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::{Type, TypeKind};
use crate::ast::Expression;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Infers the type of a cast expression (e.g., `x as float`).
    pub(crate) fn infer_cast(
        &mut self,
        value_expr: &Expression,
        target_type_expr: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let source_ty = self.infer_expression(value_expr, context);
        let target_ty = self.resolve_type_expression(target_type_expr, context);

        if !self.is_numeric_type(&source_ty.kind) {
            self.report_error(
                format!(
                    "cannot cast from non-numeric type '{}' to '{}'",
                    source_ty, target_ty
                ),
                span,
            );
            return Self::error_type();
        }

        if !self.is_numeric_type(&target_ty.kind) {
            self.report_error(
                format!(
                    "cannot cast from '{}' to non-numeric type '{}'",
                    source_ty, target_ty
                ),
                span,
            );
            return Self::error_type();
        }

        target_ty
    }

    /// Checks if a type kind is numeric (int, float, or any of their variants).
    pub(crate) fn is_numeric_type(&self, kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::Int
                | TypeKind::I8
                | TypeKind::I16
                | TypeKind::I32
                | TypeKind::I64
                | TypeKind::I128
                | TypeKind::U8
                | TypeKind::U16
                | TypeKind::U32
                | TypeKind::U64
                | TypeKind::U128
                | TypeKind::Float
                | TypeKind::F32
                | TypeKind::F64
        )
    }
}
