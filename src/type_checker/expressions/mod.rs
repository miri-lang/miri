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

use crate::ast::types::Type;
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

pub mod access;
pub mod binary;
pub mod calls;
pub mod collections;
pub mod control_flow;
pub mod functions;
pub mod identifiers;
pub mod literals;
pub mod range;
pub mod types;
pub mod unary;

impl TypeChecker {
    /// Infers the type of an expression.
    ///
    /// This is the main entry point for expression type checking. It delegates to specific
    /// handler methods based on the expression kind.
    pub(crate) fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Type {
        let ty = self.infer_expression_kind(&expr.node, expr.span, expr.id, context);
        self.types.insert(expr.id, ty.clone());
        ty
    }

    /// Dispatches expression type inference based on expression kind.
    fn infer_expression_kind(
        &mut self,
        kind: &ExpressionKind,
        span: Span,
        expr_id: usize,
        context: &mut Context,
    ) -> Type {
        match kind {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => {
                self.infer_binary(left, op, right, span, context)
            }
            ExpressionKind::Logical(left, op, right) => {
                self.infer_logical(left, op, right, span, context)
            }
            ExpressionKind::Unary(op, operand) => self.infer_unary(op, operand, span, context),
            ExpressionKind::Identifier(name, _) => self.infer_identifier(name, span, context),
            ExpressionKind::Assignment(lhs, op, rhs) => {
                self.infer_assignment(lhs, op, rhs, span, context)
            }
            ExpressionKind::Call(func, args) => {
                self.infer_call(func, args, span, context, expr_id)
            }
            ExpressionKind::Range(start, end, kind) => {
                self.infer_range(start, end, kind, span, context)
            }
            ExpressionKind::List(elements) => self.infer_list(elements, context),
            ExpressionKind::Map(entries) => self.infer_map(entries, context),
            ExpressionKind::Set(elements) => self.infer_set(elements, context),
            ExpressionKind::Tuple(elements) => self.infer_tuple(elements, context),
            ExpressionKind::Index(obj, index) => self.infer_index(obj, index, span, context),
            ExpressionKind::Member(obj, prop) => self.infer_member(obj, prop, span, context),
            ExpressionKind::Match(subject, branches) => {
                self.infer_match(subject, branches, span, context)
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                self.infer_conditional(then_expr, cond_expr, else_expr, span, context)
            }
            ExpressionKind::FormattedString(parts) => self.infer_formatted_string(parts, context),
            ExpressionKind::Lambda(lambda) => {
                self.infer_lambda(
                    &lambda.generics,
                    &lambda.params,
                    &lambda.return_type,
                    &lambda.body,
                    &lambda.properties,
                    context,
                )
            }
            ExpressionKind::TypeDeclaration(expr, generics, kind, target) => {
                self.infer_generic_instantiation(expr, generics, kind, target, span, context)
            }
            ExpressionKind::NamedArgument(_, value) => self.infer_expression(value, context),
            ExpressionKind::EnumValue(name, values) => {
                self.infer_enum_value(name, values, span, context)
            }
            ExpressionKind::Super => self.infer_super(span, context),
            ExpressionKind::Block(statements, final_expr) => {
                self.infer_block(statements, final_expr, context)
            }
            ExpressionKind::Array(elements, size) => self.infer_array(elements, size, context),
            ExpressionKind::Guard(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _) => Self::error_type(),
        }
    }

    /// Infers the type of a block expression.
    fn infer_block(
        &mut self,
        statements: &[Statement],
        final_expr: &Box<Expression>,
        context: &mut Context,
    ) -> Type {
        for stmt in statements {
            self.infer_statement_type(stmt, context);
        }
        self.infer_expression(final_expr, context)
    }
}
