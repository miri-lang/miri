// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::{Type, TypeKind};
use crate::ast::Expression;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::diagnostics::spelled;
use crate::type_checker::type_mismatch::TypeSides;
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

        // A parameter has no type to judge here, the way it has none for
        // arithmetic: the body states that it casts the parameter, and every
        // site that pins it answers for the type it pins it to — see
        // `instantiation_requirements`. The target is judged below either way,
        // because a body writes it as a type name rather than deferring it.
        let source_is_a_parameter = matches!(source_ty.kind, TypeKind::Generic(..));
        if !source_is_a_parameter && !self.casts_from(&source_ty) {
            self.refuse_cast_from(&source_ty, &target_ty, span);
            return Self::error_type();
        }

        if !self.is_numeric_type(&target_ty.kind) {
            self.refuse_cast_to(&source_ty, &target_ty, span);
            return Self::error_type();
        }

        if context.in_gpu_function {
            self.reject_device_wide_integer(&target_ty.kind, target_type_expr.span);
        }
        self.record_cast_requirement(&source_ty, &target_ty, context);
        target_ty
    }

    /// Whether a value of `source` can be cast from.
    ///
    /// The one statement of the rule, so that a site answering a cast written
    /// against a generic parameter judges the instantiation by replaying what
    /// the body itself applied rather than by forming a second opinion that
    /// would have to be kept in step with this one by hand.
    pub(crate) fn casts_from(&self, source: &Type) -> bool {
        self.is_numeric_type(&source.kind)
    }

    /// Refuses `as` from `source`, a type that holds no number.
    fn refuse_cast_from(&mut self, source: &Type, target: &Type, span: Span) {
        let help = if spelled(source) == crate::ast::types::STRING_TYPE_NAME {
            "read the number out of the text with `.to_int()` or `.to_float()`, which give \
             `None` when it holds no number"
        } else {
            "`as` converts between numeric types only; convert this value another way"
        };
        self.report_type_mismatch(
            DiagnosticCode::TypInvalidCast,
            format!(
                "cannot cast from non-numeric type `{}` to `{}`",
                spelled(source),
                spelled(target)
            ),
            span,
            TypeSides::wanting("a numeric type", source),
            help.to_string(),
        );
    }

    /// Refuses `as` to `target`, a type that holds no number.
    fn refuse_cast_to(&mut self, source: &Type, target: &Type, span: Span) {
        let help = if spelled(target) == crate::ast::types::STRING_TYPE_NAME {
            "format the value as text with an f-string, e.g. `f\"{x}\"`"
        } else {
            "`as` converts to numeric types only; build the value another way"
        };
        self.report_type_mismatch(
            DiagnosticCode::TypInvalidCast,
            format!(
                "cannot cast from `{}` to non-numeric type `{}`",
                spelled(source),
                spelled(target)
            ),
            span,
            TypeSides {
                expected: "a numeric type".to_string(),
                found: spelled(target),
            },
            help.to_string(),
        );
    }
}
