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
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
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
            // Shared Memory Validation
            if decl.is_shared {
                if !context.in_gpu_function {
                    self.report_error(
                        "Shared variables can only be declared inside 'gpu' functions".to_string(),
                        span,
                    );
                }

                if let Some(typ_expr) = &decl.typ {
                    let resolved_type = self.resolve_type_expression(typ_expr, context);
                    if !matches!(resolved_type.kind, TypeKind::Array(_, _)) {
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

            let inferred_type = self.determine_variable_type(decl, context, span);
            let is_mutable = match decl.declaration_type {
                VariableDeclarationType::Mutable => true,
                VariableDeclarationType::Immutable | VariableDeclarationType::Constant => false,
            };
            let is_constant = matches!(decl.declaration_type, VariableDeclarationType::Constant);

            self.check_shadowing(&decl.name, is_mutable, is_constant, context, span);

            if context.scopes.len() == 1 {
                self.global_scope.insert(
                    decl.name.clone(),
                    SymbolInfo::new(
                        inferred_type.clone(),
                        is_mutable,
                        is_constant,
                        visibility.clone(),
                        self.current_module.clone(),
                        None,
                    ),
                );
            }

            context.define(
                decl.name.clone(),
                SymbolInfo::new(
                    inferred_type.clone(),
                    is_mutable,
                    is_constant,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
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
    /// Warns when immutable variables are unnecessarily declared nullable.
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
                // Check for warning: assigning non-nullable to nullable immutable variable
                if let TypeKind::Nullable(_) = &declared_type.kind {
                    if !matches!(decl.declaration_type, VariableDeclarationType::Mutable) {
                        // If inferred type is NOT nullable (and not None), warn
                        if !matches!(inferred_type.kind, TypeKind::Nullable(_)) {
                            self.report_warning(
                                "W0003",
                                "Unnecessary Nullable Declaration".to_string(),
                                format!(
                                    "Unnecessary nullable declaration for variable '{}'",
                                    decl.name
                                ),
                                type_expr.span,
                                Some(format!(
                                    "Variable '{}' is immutable and its initializer is not nullable. Remove `?` from the type to simplify.",
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
