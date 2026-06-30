// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement type checking for the type checker.
//!
//! This module implements type checking for all statement kinds in Miri.
//! The main entry point is [`TypeChecker::check_statement`], which validates
//! statements and registers type information in the context.
//!
//! # Supported Statements
//!
//! ## Declarations
//! - Variable declarations: `let x = 1`, `var y: int = 2`
//! - Function declarations with generics and return type validation
//! - Struct, enum, class, and trait definitions
//! - Type aliases
//!
//! ## Control Flow
//! - If/else statements with condition type checking
//! - While loops (including forever loops)
//! - For loops with iterator type inference
//! - Match statements with exhaustiveness checking
//! - Return statements with type compatibility validation
//!
//! ## Expressions
//! - Expression statements (side effects)
//! - Assignment validation
//!
//! ## Type Definitions
//! - Structs with fields and generic parameters
//! - Enums with variants and associated values
//! - Classes with fields, methods, and inheritance
//! - Traits with method signatures
//!
//! # Return Type Analysis
//!
//! The module includes return status analysis (`check_returns`) to determine:
//! - Whether all code paths return a value
//! - Implicit vs explicit returns
//! - Return type compatibility

use crate::ast::factory::make_type;
use crate::ast::statement::BindingResidency;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::utils::{
    is_accelerable, is_gpu_compatible, resolve_element_type_kind, type_mentions_f16,
};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_variable_declaration(
        &mut self,
        decls: &[VariableDeclaration],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        for decl in decls {
            if decl.is_shared {
                self.validate_shared_variable(decl, context, span);
            }
            self.register_variable_decl(decl, visibility, context, span);
        }
    }

    fn validate_shared_variable(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
        span: Span,
    ) {
        if !context.in_gpu_function {
            self.report_error(
                "Shared variables can only be declared inside 'gpu' functions".to_string(),
                span,
            );
        }

        if let Some(typ_expr) = &decl.typ {
            let resolved_type = self.resolve_type_expression(typ_expr, context);
            let is_array = matches!(&resolved_type.kind, TypeKind::Array(_, _))
                || matches!(&resolved_type.kind, TypeKind::Custom(n, Some(_)) if BuiltinCollectionKind::from_name(n) == Some(BuiltinCollectionKind::Array));
            if !is_array {
                self.report_error(
                    format!(
                        "Shared variable '{}' must be an array, got {}",
                        decl.name, resolved_type
                    ),
                    span,
                );
            }
        } else {
            self.report_error(
                format!("Shared variable '{}' must have an explicit type", decl.name),
                span,
            );
        }

        if decl.initializer.is_some() {
            self.report_error(
                format!("Shared variable '{}' cannot have an initializer", decl.name),
                span,
            );
        }
    }

    fn register_variable_decl(
        &mut self,
        decl: &VariableDeclaration,
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        let inferred_type = self.determine_variable_type(decl, context, span);
        self.check_gpu_variable_type(&decl.name, &inferred_type, context, span);
        self.check_gpu_residency_type(decl, &inferred_type, context, span);
        self.check_host_f16(decl, &inferred_type, context, span);
        let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);
        let is_constant = matches!(decl.declaration_type, VariableDeclarationType::Constant);

        let const_value = if is_constant {
            decl.initializer.as_ref().and_then(|init| {
                Self::try_eval_const_int_with_context(init, context)
                    .map(|v| Literal::Integer(crate::ast::literal::IntegerLiteral::I128(v)))
            })
        } else {
            None
        };

        self.check_shadowing(&decl.name, is_mutable, is_constant, context, span);

        let mut info = SymbolInfo::new(
            inferred_type,
            is_mutable,
            is_constant,
            visibility.clone(),
            self.current_module.clone(),
            const_value,
        );
        info.residency = decl.residency;

        if context.scopes.len() == 1 {
            self.global_scope.insert(decl.name.clone(), info.clone());
        }
        context.define(decl.name.clone(), info);
    }

    fn check_gpu_variable_type(
        &mut self,
        name: &str,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if !context.in_gpu_function {
            return;
        }
        if matches!(inferred_type.kind, TypeKind::Error) {
            return;
        }
        if is_gpu_compatible(&inferred_type.kind) {
            return;
        }
        self.report_error(
            format!(
                "Variable '{}' has type '{}' which is not GPU-compatible: only numeric primitives, booleans, and GPU types may be used inside a 'gpu fn'",
                name, inferred_type
            ),
            span,
        );
    }

    /// Rejects a `gpu let` / `gpu var` binding whose type does not implement the
    /// `Accelerable` trait, and therefore cannot be made gpu-resident.
    ///
    /// Gating is by trait dispatch (see [`is_accelerable`]); host bindings are
    /// never constrained. Also validates literal array elements against i32 range.
    fn check_gpu_residency_type(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if decl.residency != BindingResidency::Gpu {
            return;
        }
        if matches!(inferred_type.kind, TypeKind::Error) {
            return;
        }

        if is_accelerable(&inferred_type.kind, &self.global_type_definitions) {
            self.check_gpu_i32_range_literal(decl, inferred_type, context);
            return;
        }
        self.report_error(
            format!(
                "'{}' does not implement 'Accelerable' and cannot be gpu-resident.",
                inferred_type
            ),
            span,
        );
    }

    /// Rejects an `f16` value on the host path. `f16` is a GPU-only scalar with
    /// no Cranelift representation, so it is admitted only in a `gpu let`/`gpu
    /// var` binding (gpu-resident) or inside a `gpu fn` body (a kernel-body
    /// value). A plain host `let`/`var` carrying it — directly or as a
    /// collection element — is a compile error.
    fn check_host_f16(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if decl.residency == BindingResidency::Gpu || context.in_gpu_function {
            return;
        }
        if !type_mentions_f16(&inferred_type.kind) {
            return;
        }
        self.report_error(
            format!(
                "'{}' uses 'f16', a GPU-only type with no host representation; use it inside a 'gpu' binding or a 'gpu fn'/'gpu forall'",
                inferred_type
            ),
            span,
        );
    }

    /// Checks that a gpu-resident variable with a literal integer array initializer
    /// does not contain int (i64) values outside the i32 range.
    /// Non-literal arrays and non-integer element types pass silently.
    ///
    /// This check is a fail-fast path for provably constant array elements.
    /// Runtime expressions that compute out-of-range values are caught by the
    /// narrowing validation in the runtime during buffer upload.
    fn check_gpu_i32_range_literal(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
    ) {
        let Some(init) = &decl.initializer else {
            return;
        };

        let inferred_elem_expr = match &inferred_type.kind {
            TypeKind::Array(elem_expr, _) => elem_expr.as_ref(),
            TypeKind::Custom(name, Some(args)) => {
                if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::Array) {
                    return;
                }
                if args.is_empty() {
                    return;
                }
                &args[0]
            }
            _ => return,
        };

        self.check_gpu_i32_range_array_expr(init, inferred_elem_expr, context);
    }

    /// Validates that a literal integer array expression does not contain values
    /// outside the i32 range. Used by both variable initializers and reassignments.
    /// Non-integer element types and non-literal arrays pass silently.
    /// The elem_expr is a type expression (Expression with ExpressionKind::Type or Identifier).
    pub(crate) fn check_gpu_i32_range_array_expr(
        &mut self,
        expr: &Expression,
        elem_expr: &Expression,
        context: &Context,
    ) {
        let ExpressionKind::Array(elements, _) = &expr.node else {
            return;
        };

        let elem_kind = resolve_element_type_kind(elem_expr);
        let is_int_type = matches!(elem_kind, Some(TypeKind::Int) | Some(TypeKind::I64));

        if !is_int_type {
            return;
        }

        for (elem_idx, elem) in elements.iter().enumerate() {
            if let Some(val) = Self::try_eval_const_int_with_context(elem, context) {
                if val < i32::MIN as i128 || val > i32::MAX as i128 {
                    self.report_error(
                        format!(
                            "Array element {} has value {} which exceeds i32 range [{}, {}]; \
                            use Array<i32, N> for explicit 32-bit GPU storage",
                            elem_idx,
                            val,
                            i32::MIN,
                            i32::MAX
                        ),
                        elem.span,
                    );
                }
            }
        }
    }

    pub(crate) fn check_shadowing(
        &mut self,
        name: &str,
        is_mutable: bool,
        is_constant: bool,
        context: &Context,
        span: Span,
    ) {
        // Find existing info in any scope
        let existing_info = context.resolve_info(name);

        // Rule 4: Constant shadowing is not allowed in any scope (declaring a NEW constant)
        if is_constant && existing_info.is_some() {
            self.report_error(
                format!(
                    "Cannot shadow existing variable/constant '{}' with a constant.",
                    name
                ),
                span,
            );
            return;
        }

        // Rule 5: Cannot shadow an existing constant (declaring any variable shadowing a constant)
        if let Some(existing) = existing_info {
            if existing.is_constant {
                self.report_error(format!("Cannot shadow constant '{}'.", name), span);
                return;
            }
        }

        // Check for same-scope shadowing rules
        if let Some(current_scope) = context.scopes.last() {
            if let Some(existing_info) = current_scope.get(name) {
                // Rule 2: var may not shadow in the same scope
                if is_mutable {
                    self.report_error(
                        format!("Variable '{}' is already defined in this scope. 'var' cannot shadow existing variables.", name),
                        span,
                    );
                }
                // Rule 3: switching let <-> var via shadowing in the same scope is not allowed
                // We already know new is not mutable (from Rule 2 check above), so new is 'let'.
                // If existing is 'var' (mutable), then we are switching var -> let, which is disallowed.
                else if existing_info.mutable {
                    self.report_error(
                        format!("Cannot shadow mutable variable '{}' with an immutable one in the same scope.", name),
                        span,
                    );
                }
                // Rule 1: let shadowing let is allowed (implicit else)
            }
        }
    }

    /// Determines the type of a variable from its initializer and/or type annotation.
    ///
    /// When both are present, validates compatibility and returns the declared type.
    /// Warns when immutable variables are unnecessarily declared optional.
    pub(crate) fn determine_variable_type(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
        span: Span,
    ) -> Type {
        let inferred_type = if let Some(init) = &decl.initializer {
            self.infer_expression(init, context)
        } else if let Some(type_expr) = &decl.typ {
            self.resolve_type_expression(type_expr, context)
        } else {
            self.report_error(
                format!("Cannot infer type for variable '{}'", decl.name),
                span,
            );
            make_type(TypeKind::Error)
        };

        // If both type annotation and initializer exist, check compatibility
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared_type = self.resolve_type_expression(type_expr, context);
            if !self.are_compatible(&declared_type, &inferred_type, context) {
                // Check for list literal compatibility (e.g. [1] -> [i16])
                let mut compatible = false;
                if let (TypeKind::List(target_inner), ExpressionKind::List(elements)) =
                    (&declared_type.kind, &init.node)
                {
                    if let Ok(target_inner_type) = self.extract_type_from_expression(target_inner) {
                        if self.is_integer(&target_inner_type) {
                            compatible =
                                self.check_integer_list_literal(elements, &target_inner_type);
                        }
                    }
                }

                if !compatible {
                    self.report_error(
                        format!(
                            "Type mismatch for variable '{}': expected {}, got {}",
                            decl.name, declared_type, inferred_type
                        ),
                        init.span,
                    );
                }
            } else {
                // Check for warning: assigning non-optional to optional immutable variable
                if let TypeKind::Option(_) = &declared_type.kind {
                    if !matches!(decl.declaration_type, VariableDeclarationType::Mutable) {
                        // If inferred type is NOT optional (and not None), warn
                        if !matches!(inferred_type.kind, TypeKind::Option(_)) {
                            self.report_warning(
                                "W0003",
                                "Unnecessary Optional Declaration".to_string(),
                                format!(
                                    "Unnecessary optional declaration for variable '{}'",
                                    decl.name
                                ),
                                type_expr.span,
                                Some(format!(
                                    "Variable '{}' is immutable and its initializer is not optional. Remove `?` from the type to simplify.",
                                    decl.name
                                )),
                            );
                        }
                    }
                }
            }
            return declared_type;
        }

        inferred_type
    }
}
