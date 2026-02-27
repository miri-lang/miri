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
    pub(crate) fn infer_unary(
        &mut self,
        op: &UnaryOp,
        operand: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        // Check for double negation pattern (--x)
        if matches!(op, UnaryOp::Negate) {
            if let ExpressionKind::Unary(UnaryOp::Negate, _) = &operand.node {
                self.report_warning(
                    "W0001",
                    "Unnecessary Double Negation".to_string(),
                    "Unnecessary double negation".to_string(),
                    span,
                    Some(
                        "The two negations cancel out. If this is intentional, consider simplifying to just the inner expression."
                            .to_string(),
                    ),
                );
            }
        } else if matches!(op, UnaryOp::Decrement) {
            self.report_warning(
                "W0002",
                "Decrement Operator Not Supported".to_string(),
                "Decrement operator not supported".to_string(),
                span,
                Some(
                    "`--x` is parsed as two negations (`-(-x)`), not as a decrement. Miri does not have a decrement operator — use `x = x - 1` instead."
                        .to_string(),
                ),
            );
        }

        // Validate await context: allowed outside functions or inside async functions
        if matches!(op, UnaryOp::Await) && context.in_function && !context.in_async_function {
            self.report_error(
                "'await' can only be used in async functions or at the top level".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        let expr_ty = self.infer_expression(operand, context);
        match self.check_unary_op_types(op, &expr_ty) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                ast_factory::make_type(TypeKind::Error)
            }
        }
    }
}
