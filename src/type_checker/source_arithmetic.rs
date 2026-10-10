// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The width arithmetic takes when every number in it was written in the source.
//!
//! A literal has no width of its own, and neither does a binding that names
//! one (a `const` or `let` declared without a type, bound to a number). Each
//! takes the width of the place it is used. Arithmetic built only from such
//! numbers — `2.0 * e`, `x / (2.0 * 0.04)` — has no width of its own either,
//! so it takes a width as one number: every operand in it moves to the width
//! together, and the operations compute there. Without this, `2.0 * e` beside
//! an `f32` would compute at the default `float`, and storing the result back
//! into an `f32` would be refused as a narrowing store.
//!
//! An integer expression takes a width only when every value it computes on
//! the way fits that width: `200 * 2 / 4` folds to 100, but at `u8` its
//! product would already have wrapped.

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::type_checker::context::Context;
use crate::type_checker::expressions::binary::is_arithmetic_op;
use crate::type_checker::float_literals::{is_float_width, width_for_target};
use crate::type_checker::int_literals::is_integer_width;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// The type the arithmetic `expr` takes at `width` when every number in it
    /// was written in the source, recorded on `expr` and on every operand in
    /// it; `None`, with nothing recorded, for any other expression.
    pub(crate) fn source_arithmetic_at_width(
        &mut self,
        expr: &Expression,
        width: &Type,
        context: &Context,
    ) -> Option<Type> {
        // A bare literal or binding is adapted where literals are; this is
        // for the arithmetic around them, signed or not.
        if !matches!(
            expr.node,
            ExpressionKind::Binary(..) | ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, _)
        ) {
            return None;
        }
        if !self.takes_width_whole(expr, &width.kind, context) {
            return None;
        }
        let width = Type::new(width_for_target(&width.kind, context), width.span);
        self.record_width_throughout(expr, &width);
        Some(Type::new(width.kind, expr.span))
    }

    /// Whether every number in `expr` was written in the source and every
    /// value it computes is a value of `width`.
    fn takes_width_whole(&self, expr: &Expression, width: &TypeKind, context: &Context) -> bool {
        let fits = if is_float_width(width) {
            true
        } else if is_integer_width(width) {
            folded_value_fits(expr, width, context)
        } else {
            return false;
        };
        fits && match &expr.node {
            ExpressionKind::Binary(left, op, right) => {
                is_arithmetic_op(op)
                    && self.takes_width_whole(left, width, context)
                    && self.takes_width_whole(right, width, context)
            }
            ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                self.takes_width_whole(operand, width, context)
            }
            ExpressionKind::Literal(literal) => is_number_of_family(literal, width),
            ExpressionKind::Identifier(name, _) => context.resolve_info(name).is_some_and(|info| {
                info.untyped_constant
                    && info
                        .value
                        .as_ref()
                        .is_some_and(|value| is_number_of_family(value, width))
            }),
            _ => false,
        }
    }

    /// Records `width` as the type of `expr` and of every operand inside it.
    fn record_width_throughout(&mut self, expr: &Expression, width: &Type) {
        match &expr.node {
            ExpressionKind::Binary(left, _, right) => {
                self.record_width_throughout(left, width);
                self.record_width_throughout(right, width);
            }
            ExpressionKind::Unary(_, operand) => self.record_width_throughout(operand, width),
            _ => {}
        }
        self.type_table
            .types
            .insert(expr.id, Type::new(width.kind.clone(), expr.span));
    }
}

/// Whether `literal` is a number of the family `width` belongs to: a float for
/// a float width, a whole number for an integer width.
fn is_number_of_family(literal: &Literal, width: &TypeKind) -> bool {
    match literal {
        Literal::Float(_) => is_float_width(width),
        Literal::Integer(_) => is_integer_width(width),
        Literal::String(_)
        | Literal::Boolean(_)
        | Literal::Identifier(_)
        | Literal::Regex(_)
        | Literal::None => false,
    }
}

/// Whether the integer `expr` folds to a value of `width`.
fn folded_value_fits(expr: &Expression, width: &TypeKind, context: &Context) -> bool {
    let Some(value) = TypeChecker::try_eval_const_int_with_context(expr, context) else {
        return false;
    };
    let unsigned = matches!(
        width,
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
    );
    if unsigned && value < 0 {
        return false;
    }
    IntegerLiteral::from_type_kind(width, value).is_some_and(|fit| fit.to_i128() == value)
}
