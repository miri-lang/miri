// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Use-after-move analysis for the Miri type checker.
//!
//! Tracks which variables have been "consumed" (passed as arguments to a
//! function call) and emits a compile-time error if a consumed managed-type
//! variable is subsequently accessed.
//!
//! Auto-copy types (size ≤ 128 bytes, all-primitive fields) are always copied
//! and are never flagged.

use crate::ast::expression::{ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::StatementKind;
use crate::ast::types::Type;
use crate::ast::*;
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use std::collections::HashMap;

use super::context::TypeDefinition;
use super::utils::{is_auto_copy, is_resource};

#[derive(Clone)]
struct ConsumedInfo {
    by_fn: String,
    #[allow(dead_code)]
    at_span: Span,
}

pub struct UseAfterMoveChecker<'a> {
    types: &'a HashMap<usize, Type>,
    type_definitions: &'a HashMap<String, TypeDefinition>,
    errors: Vec<TypeError>,
    /// True when analysis is inside a function body (vs. top-level script code).
    /// Resource types are always tracked. Managed types are only tracked at top level.
    in_function_body: bool,
}

impl<'a> UseAfterMoveChecker<'a> {
    pub fn new(
        types: &'a HashMap<usize, Type>,
        type_definitions: &'a HashMap<String, TypeDefinition>,
    ) -> Self {
        Self {
            types,
            type_definitions,
            errors: vec![],
            in_function_body: false,
        }
    }

    /// Runs the analysis on a complete program and returns any detected errors.
    pub fn check_program(mut self, program: &Program) -> Vec<TypeError> {
        let mut consumed: HashMap<String, ConsumedInfo> = HashMap::new();
        for stmt in &program.body {
            self.check_stmt(stmt, &mut consumed);
        }
        self.errors
    }

    fn check_stmt(&mut self, stmt: &Statement, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &stmt.node {
            // Analyze function bodies with resource-only tracking: managed types are never
            // flagged inside function bodies (read-only recursive algorithms pass the same
            // managed value to multiple calls without consuming it). Only resource types
            // (those defining `fn drop(self)`) are tracked.
            StatementKind::FunctionDeclaration(decl) => {
                if let Some(body) = &decl.body {
                    let prev = self.in_function_body;
                    self.in_function_body = true;
                    let mut fn_consumed: HashMap<String, ConsumedInfo> = HashMap::new();
                    self.check_stmt(body, &mut fn_consumed);
                    self.in_function_body = prev;
                }
            }

            StatementKind::Block(stmts) => {
                for s in stmts {
                    self.check_stmt(s, consumed);
                }
            }

            StatementKind::Expression(expr) => {
                self.check_expr(expr, consumed);
            }

            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        self.check_expr(init, consumed);
                    }
                }
            }

            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.check_expr(e, consumed);
                }
            }

            StatementKind::If(cond, then, else_, _) => {
                self.check_expr(cond, consumed);

                // Each branch gets a snapshot of the pre-branch consumed state so
                // that consuming in the then-branch doesn't poison the else-branch
                // or vice-versa.  After the if: only variables consumed in BOTH
                // branches are "definitely consumed".
                let mut then_consumed = consumed.clone();
                self.check_stmt(then, &mut then_consumed);

                let mut else_consumed = consumed.clone();
                if let Some(e) = else_ {
                    self.check_stmt(e, &mut else_consumed);
                }

                *consumed = then_consumed
                    .into_iter()
                    .filter(|(k, _)| else_consumed.contains_key(k))
                    .collect();
            }

            StatementKind::While(cond, body, _) => {
                self.check_expr(cond, consumed);
                self.check_stmt(body, consumed);
            }

            StatementKind::For(_, iter, body) => {
                self.check_expr(iter, consumed);
                self.check_stmt(body, consumed);
            }

            // Declarations, imports, type aliases — no variable uses.
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::Enum(_, _, _, _)
            | StatementKind::Struct(_, _, _, _, _)
            | StatementKind::Class(_)
            | StatementKind::Trait(_, _, _, _, _)
            | StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => {}
        }
    }

    fn check_expr(&mut self, expr: &Expression, consumed: &mut HashMap<String, ConsumedInfo>) {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => {
                if let Some(info) = consumed.get(name.as_str()) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Custom {
                            message: format!(
                                "'{}' was consumed by '{}' and cannot be used again; call .clone() to make a copy",
                                name, info.by_fn
                            ),
                            help: None,
                        },
                        span: expr.span,
                        source_override: None,
                    });
                }
            }

            ExpressionKind::Call(callee, args) => {
                let fn_name = self.extract_callee_name(callee);

                // Check callee for consumed uses (handles method receiver via Member).
                self.check_expr(callee, consumed);

                // Check args — error if any consumed variable is used.
                for arg in args {
                    self.check_expr(arg, consumed);
                }

                // After checking, mark direct Identifier (or NamedArgument wrapping one)
                // as consumed if the variable is a managed type.
                for arg in args {
                    self.maybe_consume_arg(arg, &fn_name, consumed);
                }
            }

            ExpressionKind::Member(obj, _) => {
                // Only check the object, not the field name expression.
                self.check_expr(obj, consumed);
            }

            ExpressionKind::Binary(l, _, r) | ExpressionKind::Logical(l, _, r) => {
                self.check_expr(l, consumed);
                self.check_expr(r, consumed);
            }

            ExpressionKind::Unary(_, e) => {
                self.check_expr(e, consumed);
            }

            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.check_expr(rhs, consumed);
                // Re-assigning a variable revives it (clears consumed status).
                if let LeftHandSideExpression::Identifier(name_expr) = lhs.as_ref() {
                    if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                        consumed.remove(name.as_str());
                    }
                }
            }

            ExpressionKind::Index(obj, idx) => {
                self.check_expr(obj, consumed);
                self.check_expr(idx, consumed);
            }

            ExpressionKind::Conditional(cond, then, else_, _) => {
                self.check_expr(cond, consumed);
                self.check_expr(then, consumed);
                if let Some(e) = else_ {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Range(start, end_, _) => {
                self.check_expr(start, consumed);
                if let Some(e) = end_ {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Guard(_, e) => {
                self.check_expr(e, consumed);
            }

            ExpressionKind::FormattedString(parts) => {
                for part in parts {
                    self.check_expr(part, consumed);
                }
            }

            ExpressionKind::List(elems)
            | ExpressionKind::Set(elems)
            | ExpressionKind::Tuple(elems) => {
                for e in elems {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Array(elems, _) => {
                for e in elems {
                    self.check_expr(e, consumed);
                }
            }

            ExpressionKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.check_expr(k, consumed);
                    self.check_expr(v, consumed);
                }
            }

            ExpressionKind::Match(scrutinee, branches) => {
                self.check_expr(scrutinee, consumed);

                // Each arm is an independent execution path.  A variable is
                // "definitely consumed" after a match only if it is consumed in
                // every arm; otherwise we conservatively leave it alive.
                let pre_consumed = consumed.clone();
                let mut intersection: Option<HashMap<String, ConsumedInfo>> = None;

                for branch in branches {
                    let mut arm_consumed = pre_consumed.clone();
                    if let Some(guard) = &branch.guard {
                        self.check_expr(guard, &mut arm_consumed);
                    }
                    self.check_stmt(&branch.body, &mut arm_consumed);

                    intersection = Some(match intersection.take() {
                        None => arm_consumed,
                        Some(acc) => acc
                            .into_iter()
                            .filter(|(k, _)| arm_consumed.contains_key(k))
                            .collect(),
                    });
                }

                if let Some(result) = intersection {
                    *consumed = result;
                }
            }

            ExpressionKind::EnumValue(name, values) => {
                self.check_expr(name, consumed);
                for v in values {
                    self.check_expr(v, consumed);
                }
            }

            ExpressionKind::NamedArgument(_, val) => {
                self.check_expr(val, consumed);
            }

            ExpressionKind::Lambda(lambda) => {
                // Lambda body gets its own consumed map (captures are borrowed, not consumed).
                let mut lambda_consumed = HashMap::new();
                self.check_stmt(&lambda.body, &mut lambda_consumed);
            }

            ExpressionKind::Block(stmts, final_expr) => {
                for s in stmts {
                    self.check_stmt(s, consumed);
                }
                self.check_expr(final_expr, consumed);
            }

            // Leaf nodes: literals, type expressions, super — nothing to check.
            ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Super => {}
        }
    }

    /// If `arg` is a plain `Identifier` (or a `NamedArgument` wrapping one) that refers
    /// to a variable that should be consumed, mark it as consumed by `fn_name`.
    ///
    /// At top level: any managed type is consumed (existing §7.1 behaviour).
    /// Inside a function body: only resource types (those with `fn drop(self)`) are consumed.
    fn maybe_consume_arg(
        &self,
        arg: &Expression,
        fn_name: &str,
        consumed: &mut HashMap<String, ConsumedInfo>,
    ) {
        match &arg.node {
            ExpressionKind::Identifier(name, _) => {
                if !consumed.contains_key(name.as_str()) && self.should_consume_expr(arg) {
                    consumed.insert(
                        name.clone(),
                        ConsumedInfo {
                            by_fn: fn_name.to_string(),
                            at_span: arg.span,
                        },
                    );
                }
            }
            ExpressionKind::NamedArgument(_, val) => {
                if let ExpressionKind::Identifier(name, _) = &val.node {
                    if !consumed.contains_key(name.as_str()) && self.should_consume_expr(val) {
                        consumed.insert(
                            name.clone(),
                            ConsumedInfo {
                                by_fn: fn_name.to_string(),
                                at_span: val.span,
                            },
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Returns true if passing this expression should consume it.
    ///
    /// Rule:
    /// - Resource types (`fn drop(self)` present): always consumed, at all call sites.
    /// - Other managed (non-auto-copy) types: consumed only at top level (§7.1 behaviour).
    /// - Inside a function body: only resource types are consumed.
    fn should_consume_expr(&self, expr: &Expression) -> bool {
        if let Some(ty) = self.types.get(&expr.id) {
            if is_resource(&ty.kind, self.type_definitions) {
                true
            } else if self.in_function_body {
                false
            } else {
                !is_auto_copy(&ty.kind, self.type_definitions)
            }
        } else {
            false
        }
    }

    fn extract_callee_name(&self, callee: &Expression) -> String {
        match &callee.node {
            ExpressionKind::Identifier(name, _) => name.clone(),
            ExpressionKind::Member(_, method) => {
                if let ExpressionKind::Identifier(name, _) = &method.node {
                    name.clone()
                } else {
                    "<method>".to_string()
                }
            }
            _ => "<function>".to_string(),
        }
    }
}
