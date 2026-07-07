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
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Rejects an integer literal whose value does not fit the default `int`
    /// type (`i64`), which would otherwise be silently truncated to a garbage
    /// value during MIR lowering and codegen.
    ///
    /// A literal directly under a unary negation may reach `|i64::MIN|`
    /// (`i64::MAX + 1`), since `i64::MIN` can only be spelled
    /// `-9223372036854775808`; a bare positive literal may reach only `i64::MAX`.
    pub(crate) fn check_integer_literal_range(
        &mut self,
        lit: &Literal,
        expr_id: usize,
        span: Span,
    ) {
        let Literal::Integer(int_lit) = lit else {
            return;
        };
        // A literal explicitly declared with a wider integer type keeps its full
        // i128-representable range (the parser already rejects anything larger).
        if self.wide_typed_int_literals.contains(&expr_id) {
            return;
        }
        let value = int_lit.to_i128();
        let max = if self.negated_int_literals.contains(&expr_id) {
            i64::MAX as i128 + 1
        } else {
            i64::MAX as i128
        };
        if value > max {
            self.report_error(
                format!(
                    "Integer literal '{}' is out of range for the default int type (i64, max {})",
                    value,
                    i64::MAX
                ),
                span,
            );
        }
    }

    pub(crate) fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => ast_factory::make_type(TypeKind::Int),
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => ast_factory::make_type(TypeKind::F32),
                FloatLiteral::F64(_) => ast_factory::make_type(TypeKind::F64),
            },
            Literal::Boolean(_) => ast_factory::make_type(TypeKind::Boolean),
            Literal::String(_) => ast_factory::make_type(TypeKind::String),
            Literal::Identifier(_) => ast_factory::make_type(TypeKind::Identifier),
            Literal::Regex(_) => ast_factory::make_type(TypeKind::Custom("Regex".into(), None)),
            Literal::None => ast_factory::make_type(TypeKind::Option(Box::new(
                ast_factory::make_type(TypeKind::Void),
            ))),
        }
    }

    pub(crate) fn infer_formatted_string(
        &mut self,
        parts: &[Expression],
        context: &mut Context,
    ) -> Type {
        for part in parts {
            let part_type = self.infer_expression(part, context);
            // Literal string segments are always fine; only validate interpolated expressions.
            if !matches!(&part.node, ExpressionKind::Literal(Literal::String(_)))
                && !Self::can_interpolate(&part_type.kind)
            {
                self.report_error(
                    format!(
                        "Type '{}' cannot be used in string interpolation",
                        part_type
                    ),
                    part.span,
                );
            }
        }
        make_type(TypeKind::String)
    }

    /// Returns `true` if a value of this type can be converted to a string
    /// for use in formatted string interpolation.
    fn can_interpolate(kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::String
                | TypeKind::Boolean
                | TypeKind::Int
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
                | TypeKind::Error
        )
    }
}
