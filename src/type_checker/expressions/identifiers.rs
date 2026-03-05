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
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Infers the type of an identifier reference.
    ///
    /// Handles special identifiers (`None`, `Ok`, `Err`, `self`), scope lookup,
    /// visibility checking, and linear type consumption tracking.
    pub(crate) fn infer_identifier(
        &mut self,
        name: &str,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if name == "None" {
            return ast_factory::make_type(TypeKind::Option(Box::new(ast_factory::make_type(
                TypeKind::Void,
            ))));
        }
        if name == "Some" {
            // fn<T>(value T): T?
            let t_param = ast_factory::make_type(TypeKind::Generic(
                "T".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
            let t_expr = ast_factory::type_expr_non_null(t_param.clone());

            let return_type = ast_factory::make_type(TypeKind::Option(Box::new(t_param)));

            return ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
                generics: Some(vec![t_expr.clone()]),
                params: vec![Parameter {
                    name: "value".to_string(),
                    typ: Box::new(t_expr),
                    guard: None,
                    default_value: None,
                }],
                return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
            })));
        }
        if name == "Ok" {
            // fn<T>(value T): result<T, Void>
            let t_param = ast_factory::make_type(TypeKind::Generic(
                "T".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
            let t_expr = ast_factory::type_expr_non_null(t_param.clone());
            let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

            let return_type = ast_factory::make_type(TypeKind::Result(
                Box::new(t_expr.clone()),
                Box::new(void_expr),
            ));

            return ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
                generics: Some(vec![t_expr.clone()]),
                params: vec![Parameter {
                    name: "value".to_string(),
                    typ: Box::new(t_expr),
                    guard: None,
                    default_value: None,
                }],
                return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
            })));
        }
        if name == "Err" {
            // fn<E>(error E): result<Void, E>
            let e_param = ast_factory::make_type(TypeKind::Generic(
                "E".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
            let e_expr = ast_factory::type_expr_non_null(e_param.clone());
            let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

            let return_type = ast_factory::make_type(TypeKind::Result(
                Box::new(void_expr),
                Box::new(e_expr.clone()),
            ));

            return ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
                generics: Some(vec![e_expr.clone()]),
                params: vec![Parameter {
                    name: "error".to_string(),
                    typ: Box::new(e_expr),
                    guard: None,
                    default_value: None,
                }],
                return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
            })));
        }

        // Handle 'self' keyword - refers to current class instance
        if name == "self" {
            return self.infer_self(span, context);
        }

        let info_opt = context
            .resolve_info(name)
            .cloned()
            .or_else(|| self.global_scope.get(name).cloned());

        if let Some(info) = info_opt {
            if !self.check_visibility(&info.visibility, &info.module) {
                self.report_error(format!("Variable '{}' is not visible", name), span);
                return ast_factory::make_type(TypeKind::Error);
            }

            // Linearity Check: Ensure linear resources are used exactly once
            if let TypeKind::Linear(_) = &info.ty.kind {
                if context.mark_consumed(name) {
                    self.report_error(format!("Use of moved value: '{}'", name), span);
                    return ast_factory::make_type(TypeKind::Error);
                }
            }

            return info.ty;
        }

        // Check if it is a known type (struct/enum/alias) being used as a value (constructor/meta)
        if self.global_type_definitions.contains_key(name) {
            return ast_factory::make_type(TypeKind::Meta(Box::new(ast_factory::make_type(
                TypeKind::Custom(name.to_string(), None),
            ))));
        }

        let capacity = context.scopes.iter().map(|s| s.len()).sum::<usize>() + self.global_scope.len() + 4;
        let mut candidates: Vec<&str> = Vec::with_capacity(capacity);
        for scope in &context.scopes {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.global_scope.keys().map(|s| s.as_str()));
        candidates.push("None");
        candidates.push("Some");
        candidates.push("Ok");
        candidates.push("Err");

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help(
                format!("Undefined variable: {}", name),
                span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(format!("Undefined variable: {}", name), span);
        }
        ast_factory::make_type(TypeKind::Error)
    }

    /// Infers the type of a 'self' expression.
    ///
    /// `self` refers to the current class instance. It can only be used inside a class method.
    pub(crate) fn infer_self(&mut self, span: Span, context: &Context) -> Type {
        if let Some(class_type) = &context.current_class_type {
            class_type.clone()
        } else {
            self.report_error(
                "'self' can only be used inside a class method".to_string(),
                span,
            );
            ast_factory::make_type(TypeKind::Error)
        }
    }

    /// Infers the type of a 'super' expression.
    ///
    /// `super` refers to the parent class. It can only be used inside a class that extends another.
    pub(crate) fn infer_super(&mut self, span: Span, context: &Context) -> Type {
        if context.current_class.is_none() {
            self.report_error(
                "'super' can only be used inside a class method".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        if let Some(base_class) = &context.current_base_class {
            ast_factory::make_type(TypeKind::Custom(base_class.clone(), None))
        } else {
            self.report_error(
                "'super' can only be used in a class that extends another class".to_string(),
                span,
            );
            ast_factory::make_type(TypeKind::Error)
        }
    }
}
