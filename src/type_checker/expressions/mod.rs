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
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => {
                self.infer_binary(left, op, right, expr.span, context)
            }
            ExpressionKind::Logical(left, op, right) => {
                self.infer_logical(left, op, right, expr.span, context)
            }
            ExpressionKind::Unary(op, operand) => self.infer_unary(op, operand, expr.span, context),
            ExpressionKind::Identifier(name, _) => self.infer_identifier(name, expr.span, context),
            ExpressionKind::Assignment(lhs, op, rhs) => {
                self.infer_assignment(lhs, op, rhs, expr.span, context)
            }
            ExpressionKind::Call(func, args) => self.infer_call(func, args, expr.span, context),
            ExpressionKind::Range(start, end, kind) => {
                self.infer_range(start, end, kind, expr.span, context)
            }
            ExpressionKind::List(elements) => self.infer_list(elements, context),
            ExpressionKind::Map(entries) => self.infer_map(entries, context),
            ExpressionKind::Set(elements) => self.infer_set(elements, context),
            ExpressionKind::Tuple(elements) => self.infer_tuple(elements, context),
            ExpressionKind::Index(obj, index) => self.infer_index(obj, index, expr.span, context),
            ExpressionKind::Member(obj, prop) => self.infer_member(obj, prop, expr.span, context),
            ExpressionKind::Match(subject, branches) => {
                self.infer_match(subject, branches, expr.span, context)
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                self.infer_conditional(then_expr, cond_expr, else_expr, expr.span, context)
            }
            ExpressionKind::FormattedString(parts) => self.infer_formatted_string(parts, context),
            ExpressionKind::Lambda(lambda) => self.infer_lambda(
                &lambda.generics,
                &lambda.params,
                &lambda.return_type,
                &lambda.body,
                &lambda.properties,
                context,
            ),
            ExpressionKind::TypeDeclaration(expr, generics, kind, target) => {
                self.infer_generic_instantiation(expr, generics, kind, target, expr.span, context)
            }
            ExpressionKind::NamedArgument(_, value) => self.infer_expression(value, context),
            ExpressionKind::EnumValue(name, values) => {
                self.infer_enum_value(name, values, expr.span, context)
            }
            ExpressionKind::Super => self.infer_super(expr.span, context),
            ExpressionKind::Block(statements, final_expr) => {
                // Type check all statements, then the final expression determines the type
                for stmt in statements {
                    self.infer_statement_type(stmt, context);
                }
                self.infer_expression(final_expr, context)
            }
            ExpressionKind::Array(elements, size) => self.infer_array(elements, size, context),
            // These expression kinds are handled elsewhere (parser, type expressions, etc.)
            // and should not appear as top-level inferred expressions. Guard, GenericType,
            // Type, StructMember, and ImportPath are structural AST nodes that are
            // consumed by their parent expressions during type checking.
            ExpressionKind::Guard(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _) => Self::error_type(),
        };

        self.types.insert(expr.id, ty.clone());
        ty
    }
}
