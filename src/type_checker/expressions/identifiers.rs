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
use crate::ast::types::{
    Type, TypeDeclarationKind, TypeKind, GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT,
};
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
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
        if let Some(ty) = self.try_builtin_identifier(name) {
            return ty;
        }

        if name == GPU_CONTEXT_DEPRECATED_IDENT && context.in_gpu_function {
            self.report_gpu_context_deprecation(span);
        }

        if name == "self" {
            return self.infer_self(span, context);
        }

        if let Some(ty) = self.try_variable_lookup(name, span, context) {
            return ty;
        }

        if let Some(ty) = self.try_type_constructor(name) {
            return ty;
        }

        if let Some(ty) = self.try_class_member_suggestion(name, span, context) {
            return ty;
        }

        self.report_undefined_identifier_error(name, span, context);
        ast_factory::make_type(TypeKind::Error)
    }

    fn report_gpu_context_deprecation(&mut self, span: Span) {
        self.report_warning(
            "W0004",
            "Deprecated Kernel Context Identifier".to_string(),
            format!(
                "`{}` is deprecated; use `{}` instead",
                GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT
            ),
            span,
            Some(format!(
                "Rename `{}` to `{}`. The alias is removed one release after this.",
                GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT
            )),
        );
    }

    fn try_builtin_identifier(&self, name: &str) -> Option<Type> {
        match name {
            "None" => Some(ast_factory::make_type(TypeKind::Option(Box::new(
                ast_factory::make_type(TypeKind::Void),
            )))),
            "Some" => Some(self.make_some_type()),
            "Ok" => Some(self.make_ok_type()),
            "Err" => Some(self.make_err_type()),
            // Vector-only builtins are placeholders; dispatch happens in infer_call_dispatch.
            // Exclude "mix" — it has an existing meaning (scalar math function imported from system.math).
            // Keep "length" since there is no scalar length() function, only Array.length() method.
            "dot" | "length" | "normalize" | "cross" | "reflect" => Some(ast_factory::make_type(
                TypeKind::Meta(Box::new(ast_factory::make_type(TypeKind::Void))),
            )),
            // GPU atomic operations are placeholders; dispatch happens in try_lower_atomic_builtin.
            "atomic_add"
            | "atomic_sub"
            | "atomic_max"
            | "atomic_min"
            | "atomic_and"
            | "atomic_or"
            | "atomic_xor"
            | "atomic_exchange"
            | "atomic_compare_exchange" => Some(ast_factory::make_type(TypeKind::Meta(Box::new(
                ast_factory::make_type(TypeKind::Void),
            )))),
            _ => None,
        }
    }

    fn make_some_type(&self) -> Type {
        let t_param = ast_factory::make_type(TypeKind::Generic(
            "T".to_string(),
            None,
            TypeDeclarationKind::None,
        ));
        let t_expr = ast_factory::type_expr_non_null(t_param.clone());
        let return_type = ast_factory::make_type(TypeKind::Option(Box::new(t_param)));

        ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: Some(vec![t_expr.clone()]),
            params: vec![Parameter {
                name: "value".to_string(),
                typ: Box::new(t_expr),
                guard: None,
                default_value: None,
                is_out: false,
            }],
            return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
        })))
    }

    fn make_ok_type(&self) -> Type {
        let t_param = ast_factory::make_type(TypeKind::Generic(
            "T".to_string(),
            None,
            TypeDeclarationKind::None,
        ));
        let t_expr = ast_factory::type_expr_non_null(t_param.clone());
        let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

        let return_type = ast_factory::make_type(TypeKind::Custom(
            "Result".to_string(),
            Some(vec![t_expr.clone(), void_expr]),
        ));

        ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: Some(vec![t_expr.clone()]),
            params: vec![Parameter {
                name: "value".to_string(),
                typ: Box::new(t_expr),
                guard: None,
                default_value: None,
                is_out: false,
            }],
            return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
        })))
    }

    fn make_err_type(&self) -> Type {
        let e_param = ast_factory::make_type(TypeKind::Generic(
            "E".to_string(),
            None,
            TypeDeclarationKind::None,
        ));
        let e_expr = ast_factory::type_expr_non_null(e_param.clone());
        let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

        let return_type = ast_factory::make_type(TypeKind::Custom(
            "Result".to_string(),
            Some(vec![void_expr, e_expr.clone()]),
        ));

        ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: Some(vec![e_expr.clone()]),
            params: vec![Parameter {
                name: "error".to_string(),
                typ: Box::new(e_expr),
                guard: None,
                default_value: None,
                is_out: false,
            }],
            return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
        })))
    }

    fn try_variable_lookup(
        &mut self,
        name: &str,
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        let info_opt = context
            .resolve_info(name)
            .cloned()
            .or_else(|| self.global_scope.get(name).cloned());

        if let Some(info) = info_opt {
            if !self.check_visibility(&info.visibility, &info.module) {
                let kind = if self.global_type_definitions.contains_key(name) {
                    "Type"
                } else if matches!(info.ty.kind, TypeKind::Function(_)) {
                    "Function"
                } else {
                    "Variable"
                };
                self.report_error(format!("{} '{}' is not visible", kind, name), span);
                return Some(ast_factory::make_type(TypeKind::Error));
            }

            if let TypeKind::Linear(_) = &info.ty.kind {
                if context.mark_consumed(name) {
                    self.report_error(format!("Use of moved value: '{}'", name), span);
                    return Some(ast_factory::make_type(TypeKind::Error));
                }
            }

            return Some(info.ty);
        }

        None
    }

    fn try_type_constructor(&self, name: &str) -> Option<Type> {
        if self.is_type_visible(name) {
            Some(ast_factory::make_type(TypeKind::Meta(Box::new(
                ast_factory::make_type(TypeKind::Custom(name.to_string(), None)),
            ))))
        } else {
            None
        }
    }

    fn try_class_member_suggestion(
        &mut self,
        name: &str,
        span: Span,
        context: &Context,
    ) -> Option<Type> {
        if let Some(class_name) = &context.current_class {
            if let Some((member_kind, hint)) = self.find_self_member_hint(name, class_name) {
                self.report_error_with_help(
                    format!("Undefined {}: {}", member_kind, name),
                    span,
                    hint,
                );
                return Some(ast_factory::make_type(TypeKind::Error));
            }
        }
        None
    }

    fn report_undefined_identifier_error(&mut self, name: &str, span: Span, context: &Context) {
        let entity_kind = if self.global_type_definitions.contains_key(name)
            || name.starts_with(|c: char| c.is_uppercase())
        {
            "type"
        } else {
            "variable"
        };

        let capacity = context.scopes.iter().map(|s| s.len()).sum::<usize>()
            + self.global_scope.len()
            + self.global_type_definitions.len();
        let mut candidates: Vec<&str> = Vec::with_capacity(capacity);
        for scope in &context.scopes {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.global_scope.keys().map(|s| s.as_str()));
        candidates.extend(self.visible_type_names.iter().map(|s| s.as_str()));

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help(
                format!("Undefined {}: {}", entity_kind, name),
                span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(format!("Undefined {}: {}", entity_kind, name), span);
        }
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

    /// Checks if `name` matches a method or field on the given class (or its base classes).
    /// Returns `(entity_kind, hint)` — e.g. `("method", "Did you mean 'self.name()'?")`.
    fn find_self_member_hint(
        &self,
        name: &str,
        class_name: &str,
    ) -> Option<(&'static str, String)> {
        let mut current = class_name.to_string();
        loop {
            let def = self.global_type_definitions.get(&current)?;
            if let TypeDefinition::Class(class_def) = def {
                if class_def.methods.contains_key(name) {
                    return Some(("method", format!("Did you mean 'self.{}()'?", name)));
                }
                if class_def.fields.iter().any(|(n, _)| n == name) {
                    return Some(("field", format!("Did you mean 'self.{}'?", name)));
                }
                if let Some(base) = &class_def.base_class {
                    current = base.clone();
                } else {
                    return None;
                }
            } else {
                return None;
            }
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
