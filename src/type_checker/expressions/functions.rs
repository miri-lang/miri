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
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn infer_lambda(
        &mut self,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        body: &Statement,
        _properties: &FunctionProperties,
        context: &mut Context,
    ) -> Type {
        context.enter_scope();

        self.resolve_lambda_generics(generics, context);
        let expected_return_type = self.setup_return_type_context(return_type_expr, context);
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        self.define_lambda_parameters(params, context);

        let implicit_return_type = self.infer_lambda_body(body, context);
        let final_return_type_expr = self.finalize_lambda_return_type(
            return_type_expr,
            expected_return_type,
            implicit_return_type,
            body,
            context,
        );

        if return_type_expr.is_some() {
            context.return_types.pop();
            context.inferred_return_types.pop();
        }

        context.loop_depth = old_loop_depth;
        context.exit_scope();

        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
            params: params.to_vec(),
            return_type: final_return_type_expr,
        })))
    }

    fn resolve_lambda_generics(
        &mut self,
        generics: &Option<Vec<Expression>>,
        context: &mut Context,
    ) {
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }
    }

    fn setup_return_type_context(
        &mut self,
        return_type_expr: &Option<Box<Expression>>,
        context: &mut Context,
    ) -> Option<Type> {
        let expected_return_type = return_type_expr
            .as_ref()
            .map(|rt_expr| self.resolve_type_expression(rt_expr, context));

        if let Some(rt) = &expected_return_type {
            context.return_types.push(rt.clone());
            context.inferred_return_types.push(None);
        } else {
            context.return_types.push(make_type(TypeKind::Void));
            context.inferred_return_types.push(Some(Vec::new()));
        }

        expected_return_type
    }

    fn define_lambda_parameters(&mut self, params: &[Parameter], context: &mut Context) {
        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);
            context.define(
                param.name.clone(),
                SymbolInfo::new(
                    param_type,
                    false,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
        }
    }

    fn infer_lambda_body(&mut self, body: &Statement, context: &mut Context) -> Type {
        match &body.node {
            StatementKind::Block(stmts) => {
                context.enter_scope();
                let mut last_type = make_type(TypeKind::Void);
                for (i, stmt) in stmts.iter().enumerate() {
                    if i == stmts.len() - 1 {
                        if let StatementKind::Expression(expr) = &stmt.node {
                            last_type = self.infer_expression(expr, context);
                        } else {
                            self.check_statement(stmt, context);
                        }
                    } else {
                        self.check_statement(stmt, context);
                    }
                }
                context.exit_scope();
                last_type
            }
            StatementKind::Expression(expr) => self.infer_expression(expr, context),
            _ => {
                self.check_statement(body, context);
                make_type(TypeKind::Void)
            }
        }
    }

    fn finalize_lambda_return_type(
        &mut self,
        return_type_expr: &Option<Box<Expression>>,
        expected_return_type: Option<Type>,
        implicit_return_type: Type,
        body: &Statement,
        context: &mut Context,
    ) -> Option<Box<Expression>> {
        if let Some(expected) = expected_return_type {
            self.validate_explicit_return_type(&expected, &implicit_return_type, body, context);
            return_type_expr.clone()
        } else {
            let collected_returns = context
                .inferred_return_types
                .pop()
                .unwrap_or_else(|| Some(Vec::new()))
                .unwrap_or_default();
            context.return_types.pop();

            let candidate =
                self.unify_inferred_returns(implicit_return_type, collected_returns, context);
            Some(Box::new(self.create_type_expression(candidate)))
        }
    }

    fn validate_explicit_return_type(
        &mut self,
        expected: &Type,
        implicit_return_type: &Type,
        body: &Statement,
        context: &mut Context,
    ) {
        let is_void_implicit = matches!(implicit_return_type.kind, TypeKind::Void);
        let is_void_expected = matches!(expected.kind, TypeKind::Void);
        let is_error = matches!(expected.kind, TypeKind::Error)
            || matches!(implicit_return_type.kind, TypeKind::Error);

        if is_error {
            return;
        }

        if !is_void_expected && is_void_implicit {
            let ends_with_return = self.body_ends_with_return(body);
            if !ends_with_return {
                self.report_error(
                    format!(
                        "Invalid return type: expected {}, got {}",
                        expected, implicit_return_type
                    ),
                    body.span,
                );
            }
        } else if !self.are_compatible(expected, implicit_return_type, context)
            && !matches!(expected.kind, TypeKind::Void)
        {
            self.report_error(
                format!(
                    "Invalid return type: expected {}, got {}",
                    expected, implicit_return_type
                ),
                body.span,
            );
        }
    }

    fn body_ends_with_return(&self, body: &Statement) -> bool {
        match &body.node {
            StatementKind::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    matches!(last.node, StatementKind::Return(_))
                } else {
                    false
                }
            }
            StatementKind::Return(_) => true,
            _ => false,
        }
    }

    fn unify_inferred_returns(
        &mut self,
        mut candidate: Type,
        collected_returns: Vec<(Type, Span)>,
        context: &mut Context,
    ) -> Type {
        for (ret_ty, ret_span) in collected_returns {
            if matches!(candidate.kind, TypeKind::Void) {
                candidate = ret_ty;
            } else if !matches!(ret_ty.kind, TypeKind::Void) {
                if !self.are_compatible(&candidate, &ret_ty, context) {
                    self.report_error(
                        format!(
                            "Incompatible return types in lambda: {} and {}",
                            candidate, ret_ty
                        ),
                        ret_span,
                    );
                }
            } else {
                self.report_error(
                    format!(
                        "Incompatible return types in lambda: {} and {}",
                        candidate, ret_ty
                    ),
                    ret_span,
                );
            }
        }
        candidate
    }
}
