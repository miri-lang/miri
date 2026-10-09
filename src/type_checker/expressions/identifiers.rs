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
use crate::diagnostics::DiagnosticCode;
use crate::diagnostics::RepairRequest;
use crate::error::foreign_syntax::ForeignForm;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo, TypeDefinition};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Infers the type of an identifier reference.
    ///
    /// Handles special identifiers (`None`, `Ok`, `Err`, `self`), scope lookup,
    /// visibility checking, and linear type consumption tracking. A reference
    /// that resolves to a `runtime` or `intrinsic` declaration is recorded
    /// under `expr_id`.
    pub(crate) fn infer_identifier(
        &mut self,
        name: &str,
        span: Span,
        expr_id: usize,
        context: &mut Context,
    ) -> Type {
        if let Some(ty) = self.try_builtin_identifier(name) {
            // A built-in that is a function is a variant constructor: `Some`,
            // `Ok`, `Err`.
            if matches!(ty.kind, TypeKind::Function(_)) {
                self.fn_analysis
                    .callee_kinds
                    .insert(expr_id, crate::type_checker::CalleeKind::VariantConstructor);
            }
            return ty;
        }

        if name == GPU_CONTEXT_DEPRECATED_IDENT && context.in_gpu_function {
            self.report_gpu_context_deprecation(span);
        }

        if name == "self" {
            return self.infer_self(span, context);
        }

        if let Some(ty) = self.try_variable_lookup(name, span, expr_id, context) {
            self.record_type_named_by(expr_id, &ty);
            return ty;
        }

        if let Some(ty) = self.try_type_constructor(name) {
            self.record_type_named_by(expr_id, &ty);
            return ty;
        }

        if let Some(ty) = self.try_class_member_suggestion(name, span, context) {
            return ty;
        }

        if self.modules.is_kept_private_elsewhere(name) {
            self.report_error(
                DiagnosticCode::TypNameNotVisible,
                format!("Type '{name}' is not visible"),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        self.report_undefined_identifier_error(name, span, context);
        ast_factory::make_type(TypeKind::Error)
    }

    fn report_gpu_context_deprecation(&mut self, span: Span) {
        self.report_warning(
            DiagnosticCode::NamDeprecatedKernelContextIdentifier,
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
                name_span: Default::default(),
                name: "value".to_string(),
                typ: Box::new(t_expr),
                guard: None,
                default_value: None,
                is_out: false,
                residency: None,
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
                name_span: Default::default(),
                name: "value".to_string(),
                typ: Box::new(t_expr),
                guard: None,
                default_value: None,
                is_out: false,
                residency: None,
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
                name_span: Default::default(),
                name: "error".to_string(),
                typ: Box::new(e_expr),
                guard: None,
                default_value: None,
                is_out: false,
                residency: None,
            }],
            return_type: Some(Box::new(ast_factory::type_expr_non_null(return_type))),
        })))
    }

    fn try_variable_lookup(
        &mut self,
        name: &str,
        span: Span,
        expr_id: usize,
        context: &mut Context,
    ) -> Option<Type> {
        let info_opt = self.resolve_value_name(name, context).cloned();
        if info_opt.as_ref().is_some_and(declares_type_parameters)
            && self.callee_expr_id != Some(expr_id)
        {
            self.refuse_generic_function_value(name, span);
            return Some(ast_factory::make_type(TypeKind::Error));
        }

        if let Some(info) = info_opt {
            self.record_callee(expr_id, name, &info);
            if !self.check_visibility(&info.visibility, &info.module) {
                let names_a_type = matches!(info.ty.kind, TypeKind::Meta(_))
                    || self.type_table.global_type_definitions.contains_key(name);
                let kind = if names_a_type {
                    "Type"
                } else if matches!(info.ty.kind, TypeKind::Function(_)) {
                    "Function"
                } else {
                    "Variable"
                };
                self.report_error(
                    DiagnosticCode::TypNameNotVisible,
                    format!("{} '{}' is not visible", kind, name),
                    span,
                );
                return Some(ast_factory::make_type(TypeKind::Error));
            }

            if let TypeKind::Linear(_) = &info.ty.kind {
                if context.mark_consumed(name) {
                    self.report_error(
                        DiagnosticCode::OwnUseOfMovedValue,
                        format!("Use of moved value: '{}'", name),
                        span,
                    );
                    return Some(ast_factory::make_type(TypeKind::Error));
                }
            }

            return Some(info.ty);
        }

        None
    }

    /// The binding `name` names: a local or any other scoped binding first, as
    /// for every name, then the module's own private type of that name, then a
    /// global one. A type the module keeps private is registered under its
    /// identity, so a binding written over its name shadows it.
    fn resolve_value_name<'a>(
        &'a self,
        name: &'a str,
        context: &'a Context,
    ) -> Option<&'a SymbolInfo> {
        let key = self.written_type_identity_in(name, context);
        context
            .resolve_info(name)
            .or_else(|| context.resolve_info(key))
            .or_else(|| self.type_table.global_scope.get(key))
    }

    fn try_type_constructor(&self, name: &str) -> Option<Type> {
        if self.is_type_visible(name) {
            let identity = self.written_type_identity(name).to_string();
            Some(ast_factory::make_type(TypeKind::Meta(Box::new(
                ast_factory::make_type(TypeKind::Custom(identity, None)),
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
                    DiagnosticCode::TypUndefinedName,
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
        // A name another language spells the absent value with resolves nowhere,
        // so it arrives here as an ordinary unknown name. Naming the construct
        // is more use than guessing at the nearest binding in scope.
        if let Some(form) = foreign_absent_value(name, span) {
            if let Some(repair) = form.repair() {
                self.report_error_with_help_and_repair(
                    DiagnosticCode::TypUndefinedName,
                    format!("Undefined variable: {}", name),
                    span,
                    form.help().to_string(),
                    repair,
                );
                return;
            }
        }

        let entity_kind = if self.type_table.global_type_definitions.contains_key(name)
            || name.starts_with(|c: char| c.is_uppercase())
        {
            "type"
        } else {
            "variable"
        };

        // A capitalized identifier that names an unimported stdlib type (e.g.
        // `List` used without `system.collections.list`) gets the unified
        // import hint rather than a nearest-name guess.
        if entity_kind == "type" && self.report_hidden_type_import_hint(name, span) {
            return;
        }

        let capacity = context.scopes.iter().map(|s| s.len()).sum::<usize>()
            + self.type_table.global_scope.len()
            + self.type_table.global_type_definitions.len();
        let mut candidates: Vec<&str> = Vec::with_capacity(capacity);
        for scope in &context.scopes {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.type_table.global_scope.keys().map(|s| s.as_str()));
        candidates.extend(
            self.type_table
                .visible_type_names
                .iter()
                .map(|s| s.as_str()),
        );

        let message = format!("Undefined {}: {}", entity_kind, name);

        // A name declared by exactly one module is repaired by importing it.
        // Two or more candidates is a choice the author has to make, so those
        // keep the diagnostic and get no repair.
        if let [module] = Self::modules_declaring_function(name).as_slice() {
            self.report_error_with_repair(
                DiagnosticCode::TypUndefinedName,
                message,
                span,
                RepairRequest::AddImport {
                    module: module.clone(),
                    name: name.to_string(),
                },
            );
            return;
        }

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help_and_optional_repair(
                DiagnosticCode::TypUndefinedName,
                message,
                span,
                format!("Did you mean '{}'?", suggestion),
                RepairRequest::rename(span.start, span.end, name, &suggestion),
            );
        } else {
            self.report_error(DiagnosticCode::TypUndefinedName, message, span);
        }
    }

    /// Infers the type of a 'self' expression.
    ///
    /// `self` refers to the current class instance. It can only be used inside a class method.
    pub(crate) fn infer_self(&mut self, span: Span, context: &Context) -> Type {
        if context.in_static_method {
            self.report_error(
                DiagnosticCode::TypStaticMethodRestriction,
                "'self' cannot be used in static methods".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        if let Some(class_type) = &context.current_class_type {
            class_type.clone()
        } else {
            self.report_error(
                DiagnosticCode::TypClassDefinition,
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
            let def = self.type_table.global_type_definitions.get(&current)?;
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
    /// `super` refers to the parent class at the arguments the `extends`
    /// clause gives it, so a member reached through it is typed at what the
    /// clause binds the parent's parameters to. It can only be used inside a
    /// class that extends another.
    pub(crate) fn infer_super(&mut self, span: Span, context: &Context) -> Type {
        if context.current_class.is_none() {
            self.report_error(
                DiagnosticCode::TypClassDefinition,
                "'super' can only be used inside a class method".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        if let Some(base_class) = &context.current_base_class {
            let clause_args = self.extends_clause_arguments(context).map(|args| {
                args.iter()
                    .cloned()
                    .map(ast_factory::type_expr_non_null)
                    .collect()
            });
            ast_factory::make_type(TypeKind::Custom(base_class.clone(), clause_args))
        } else {
            self.report_error(
                DiagnosticCode::TypClassDefinition,
                "'super' can only be used in a class that extends another class".to_string(),
                span,
            );
            ast_factory::make_type(TypeKind::Error)
        }
    }

    /// The type arguments the current class's `extends` clause gives its
    /// parent, or `None` when the clause names none.
    fn extends_clause_arguments<'a>(&'a self, context: &'a Context) -> Option<&'a [Type]> {
        let class_name = context.current_class.as_deref()?;
        let TypeDefinition::Class(class_def) = context
            .resolve_type_definition(class_name)
            .or_else(|| self.type_table.global_type_definitions.get(class_name))?
        else {
            return None;
        };
        class_def.base_class_args.as_deref()
    }
}

/// The absent-value spelling `name` comes from, when it is not Miri's.
///
/// Miri writes the absent value as a literal, so these names never resolve to
/// anything and would otherwise be measured against the bindings in scope for a
/// nearest match that cannot exist.
fn foreign_absent_value(name: &str, span: Span) -> Option<ForeignForm> {
    FOREIGN_ABSENT_VALUES
        .contains(&name)
        .then_some(ForeignForm::NullLiteral {
            spelling_start: span.start,
            spelling_end: span.end,
        })
}

/// The names other languages give the absent value.
const FOREIGN_ABSENT_VALUES: &[&str] = &["null", "nil", "nullptr"];

/// Whether `info` is a function that declares type parameters of its own.
pub(crate) fn declares_type_parameters(info: &crate::type_checker::context::SymbolInfo) -> bool {
    matches!(
        &info.ty.kind,
        TypeKind::Function(function)
            if function.generics.as_ref().is_some_and(|generics| !generics.is_empty())
    )
}

impl TypeChecker {
    /// Refuse the generic function `name` used as a value. Only a call binds
    /// its type parameters; a value reaching no call would be compiled for no
    /// caller's types.
    pub(crate) fn refuse_generic_function_value(&mut self, name: &str, span: Span) {
        self.report_error(
            DiagnosticCode::TypTypeInference,
            format!(
                "Cannot use the generic function '{name}' as a value: its type parameters \
                 are bound only where it is called. Call it, or wrap the call in a lambda \
                 with concrete types — `fn(x int) int: {name}(x)`"
            ),
            span,
        );
    }
}
