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

use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn infer_range(
        &mut self,
        start: &Expression,
        end: &Option<Box<Expression>>,
        kind: &RangeExpressionType,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let start_type = self.infer_expression(start, context);

        if matches!(kind, RangeExpressionType::IterableObject) {
            return start_type;
        }

        if let Some(end_expr) = end {
            let end_type = self.infer_expression(end_expr, context);
            if !self.are_compatible(&start_type, &end_type, context) {
                self.report_error(
                    format!("Range types mismatch: {} and {}", start_type, end_type),
                    span,
                );
            }
        }

        let type_expr = self.create_type_expression(start_type);
        make_type(TypeKind::Custom("Range".to_string(), Some(vec![type_expr])))
    }
}
