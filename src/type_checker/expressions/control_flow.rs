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
use crate::type_checker::context::{Context, SymbolInfo, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

impl TypeChecker {
    /// Infers the type of a match expression.
    ///
    /// Validates exhaustiveness for enum subjects, checks pattern types,
    /// and ensures all branch bodies produce compatible types.
    pub(crate) fn infer_match(
        &mut self,
        subject: &Expression,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let subject_type = self.infer_expression(subject, context);

        // Check exhaustiveness for Enums
        if let TypeKind::Custom(name, _) = &subject_type.kind {
            // Find enum definition
            let mut enum_def_opt = None;

            // Check local scopes first (reverse order)
            for scope in context.type_definitions.iter().rev() {
                if let Some(TypeDefinition::Enum(def)) = scope.get(name) {
                    enum_def_opt = Some(def);
                    break;
                }
            }

            // Check global scope if not found locally
            if enum_def_opt.is_none() {
                if let Some(TypeDefinition::Enum(def)) = self.global_type_definitions.get(name) {
                    enum_def_opt = Some(def);
                }
            }

            if let Some(enum_def) = enum_def_opt {
                let mut remaining_variants: HashSet<String> =
                    enum_def.variants.keys().cloned().collect();
                let mut is_exhaustive = false;

                for branch in branches {
                    // Only unguarded patterns count toward exhaustiveness
                    if branch.guard.is_none() {
                        for pattern in &branch.patterns {
                            match pattern {
                                Pattern::Default => {
                                    is_exhaustive = true;
                                }
                                Pattern::Identifier(_) => {
                                    // Variable binding covers everything
                                    is_exhaustive = true;
                                }
                                Pattern::Member(parent, member) => {
                                    // Check if parent is the enum name
                                    if let Pattern::Identifier(parent_name) = &**parent {
                                        if parent_name == name {
                                            remaining_variants.remove(member);
                                        }
                                    }
                                }
                                Pattern::EnumVariant(parent, _) => {
                                    if let Pattern::Member(enum_name_pat, variant_name) = &**parent
                                    {
                                        if let Pattern::Identifier(enum_name_str) = &**enum_name_pat
                                        {
                                            if enum_name_str == name {
                                                remaining_variants.remove(variant_name);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if is_exhaustive {
                        break;
                    }
                }

                if !is_exhaustive && !remaining_variants.is_empty() {
                    let mut missing: Vec<_> = remaining_variants.into_iter().collect();
                    missing.sort();
                    self.report_error(
                        format!(
                            "Non-exhaustive match on Enum '{}'. Missing variants: {}",
                            name,
                            missing.join(", ")
                        ),
                        span,
                    );
                }
            }
        }

        // Check exhaustiveness for Option types
        if matches!(subject_type.kind, TypeKind::Option(_)) {
            let mut has_some = false;
            let mut has_none = false;
            let mut is_exhaustive = false;

            for branch in branches {
                if branch.guard.is_none() {
                    for pattern in &branch.patterns {
                        match pattern {
                            Pattern::Default => {
                                is_exhaustive = true;
                            }
                            Pattern::Literal(crate::ast::literal::Literal::None) => {
                                has_none = true;
                            }
                            Pattern::Identifier(_) => {
                                // Variable binding covers everything
                                is_exhaustive = true;
                            }
                            Pattern::Member(parent, member) => {
                                if let Pattern::Identifier(parent_name) = &**parent {
                                    if parent_name == "Option" {
                                        match member.as_str() {
                                            "Some" => has_some = true,
                                            "None" => has_none = true,
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            Pattern::EnumVariant(parent, _) => match &**parent {
                                Pattern::Identifier(name) if name == "Some" => {
                                    has_some = true;
                                }
                                Pattern::Member(enum_pat, variant) => {
                                    if let Pattern::Identifier(name) = &**enum_pat {
                                        if name == "Option" && variant == "Some" {
                                            has_some = true;
                                        }
                                    }
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
                if is_exhaustive {
                    break;
                }
            }

            if !(is_exhaustive || has_some && has_none) {
                let mut missing = Vec::new();
                if !has_some {
                    missing.push("Some");
                }
                if !has_none {
                    missing.push("None");
                }
                self.report_error(
                    format!(
                        "Non-exhaustive match on Option. Missing variants: {}",
                        missing.join(", ")
                    ),
                    span,
                );
            }
        }

        if branches.is_empty() {
            return make_type(TypeKind::Void);
        }

        let mut first_branch_type = None;

        for branch in branches {
            context.enter_scope();
            for pattern in &branch.patterns {
                self.check_pattern(pattern, &subject_type, context, span, branch.is_mutable);
            }

            let body_type = self.infer_statement_type(&branch.body, context);
            context.exit_scope();

            if let Some(first) = &first_branch_type {
                if !self.are_compatible(first, &body_type, context) {
                    self.report_error(
                        format!(
                            "Match branch types mismatch: expected {}, got {}",
                            first, body_type
                        ),
                        span,
                    );
                }
            } else {
                first_branch_type = Some(body_type);
            }
        }

        first_branch_type.unwrap_or(make_type(TypeKind::Void))
    }

    pub(crate) fn infer_conditional(
        &mut self,
        then_expr: &Expression,
        cond_expr: &Expression,
        else_expr_opt: &Option<Box<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let cond_type = self.infer_expression(cond_expr, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("Conditional condition must be a boolean, got {}", cond_type),
                cond_expr.span,
            );
        }

        let then_type = self.infer_expression(then_expr, context);

        if let Some(else_expr) = else_expr_opt {
            let else_type = self.infer_expression(else_expr, context);
            if !self.are_compatible(&then_type, &else_type, context) {
                self.report_error(
                    format!(
                        "Conditional branches must have the same type: expected {}, got {}",
                        then_type, else_type
                    ),
                    span,
                );
            }
            then_type
        } else {
            if !self.are_compatible(&then_type, &make_type(TypeKind::Void), context) {
                self.report_error(
                    format!(
                        "Conditional expression without else branch must return Void, got {}",
                        then_type
                    ),
                    span,
                );
            }
            make_type(TypeKind::Void)
        }
    }

    /// Validates a match pattern against the expected subject type.
    ///
    /// Binds pattern variables in the current scope and validates:
    /// - Literal patterns against subject type
    /// - Tuple destructuring with element count
    /// - Enum variant patterns with binding count and generic substitution
    /// - Regex patterns against string subjects
    pub(crate) fn check_pattern(
        &mut self,
        pattern: &Pattern,
        subject_type: &Type,
        context: &mut Context,
        span: Span,
        is_mutable: bool,
    ) {
        match pattern {
            Pattern::Literal(lit) => {
                // None literal on Option subject is the None variant match
                if matches!(lit, crate::ast::literal::Literal::None)
                    && matches!(subject_type.kind, TypeKind::Option(_))
                {
                    return;
                }
                let lit_type = self.infer_literal(lit);
                if !self.are_compatible(subject_type, &lit_type, context) {
                    self.report_error(
                        format!(
                            "Pattern type mismatch: expected {}, got {}",
                            subject_type, lit_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Identifier(name) => {
                // Bind variable (mutable when `var` was used)
                context.define(
                    name.clone(),
                    SymbolInfo::new(
                        subject_type.clone(),
                        is_mutable,
                        false,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                        None,
                    ),
                );
            }
            Pattern::Tuple(patterns) => {
                if let TypeKind::Tuple(elem_types) = &subject_type.kind {
                    if patterns.len() != elem_types.len() {
                        self.report_error(
                            format!(
                                "Tuple pattern length mismatch: expected {}, got {}",
                                elem_types.len(),
                                patterns.len()
                            ),
                            span,
                        );
                        return;
                    }

                    // Clone to avoid borrowing issues
                    let elem_types_cloned = elem_types.clone();

                    for (i, pat) in patterns.iter().enumerate() {
                        let elem_type =
                            self.resolve_type_expression(&elem_types_cloned[i], context);
                        self.check_pattern(pat, &elem_type, context, span, is_mutable);
                    }
                } else {
                    self.report_error(
                        format!(
                            "Expected tuple type for tuple pattern, got {}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Member(parent, member) => {
                // Option.None pattern
                if let Pattern::Identifier(parent_name) = &**parent {
                    if parent_name == "Option"
                        && member == "None"
                        && matches!(subject_type.kind, TypeKind::Option(_))
                    {
                        return;
                    }
                }
                if let Pattern::Identifier(parent_name) = &**parent {
                    let enum_def_opt = self.resolve_visible_type(parent_name, context).cloned();
                    if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                        if !enum_def.variants.contains_key(member) {
                            self.report_error(
                                format!("Enum '{}' has no variant '{}'", parent_name, member),
                                span,
                            );
                        }
                        // Check if subject type matches the enum type
                        // We construct the expected type from the enum name, preserving generic args if present in subject
                        let expected_type = if let TypeKind::Custom(sub_name, sub_args) =
                            &subject_type.kind
                        {
                            if sub_name == parent_name {
                                make_type(TypeKind::Custom(parent_name.clone(), sub_args.clone()))
                            } else {
                                make_type(TypeKind::Custom(parent_name.clone(), None))
                            }
                        } else {
                            make_type(TypeKind::Custom(parent_name.clone(), None))
                        };
                        if !self.are_compatible(subject_type, &expected_type, context) {
                            self.report_error(
                                format!(
                                    "Pattern type mismatch: expected {}, got {}",
                                    subject_type, expected_type
                                ),
                                span,
                            );
                        }
                    } else {
                        self.report_error(format!("'{}' is not an Enum", parent_name), span);
                    }
                } else {
                    self.report_error(
                        "Complex member patterns are not supported".to_string(),
                        span,
                    );
                }
            }
            Pattern::Regex(_) => {
                if !matches!(subject_type.kind, TypeKind::String) {
                    self.report_error(
                        format!(
                            "Regex pattern requires string subject, got {}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Default => {}
            Pattern::EnumVariant(parent_pattern, bindings) => {
                // Handle Option patterns: Some(x) or Option.Some(x)
                if matches!(subject_type.kind, TypeKind::Option(_)) {
                    let is_option_some = match &**parent_pattern {
                        // Some(x) — bare identifier
                        Pattern::Identifier(name) => name == "Some",
                        // Option.Some(x) — qualified
                        Pattern::Member(enum_pat, variant) => {
                            if let Pattern::Identifier(name) = &**enum_pat {
                                name == "Option" && variant == "Some"
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if is_option_some {
                        if bindings.len() != 1 {
                            self.report_error(
                                format!("Some pattern expects 1 binding, got {}", bindings.len()),
                                span,
                            );
                            return;
                        }
                        if let TypeKind::Option(inner) = &subject_type.kind {
                            let inner_type = inner.as_ref().clone();
                            self.check_pattern(
                                &bindings[0],
                                &inner_type,
                                context,
                                span,
                                is_mutable,
                            );
                        }
                        return;
                    }
                }

                // Extract enum name and variant name from parent pattern
                let (enum_name, variant_name) = match &**parent_pattern {
                    Pattern::Member(enum_pat, variant) => {
                        if let Pattern::Identifier(name) = &**enum_pat {
                            (name.clone(), variant.clone())
                        } else {
                            self.report_error(
                                "Complex member patterns are not supported".to_string(),
                                span,
                            );
                            return;
                        }
                    }
                    Pattern::Identifier(name) => {
                        // Could be just a variant if subject type is known enum
                        self.report_error(
                            format!("Expected enum variant pattern like EnumName.{}", name),
                            span,
                        );
                        return;
                    }
                    _ => {
                        self.report_error("Invalid enum variant pattern".to_string(), span);
                        return;
                    }
                };

                // Look up enum definition (must be visible in scope)
                let enum_def_opt = self.resolve_visible_type(&enum_name, context).cloned();
                if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                    if let Some(variant_types) = enum_def.variants.get(&variant_name) {
                        // Check binding count matches
                        if bindings.len() != variant_types.len() {
                            self.report_error(
                                format!(
                                    "Enum variant '{}' expects {} bindings, got {}",
                                    variant_name,
                                    variant_types.len(),
                                    bindings.len()
                                ),
                                span,
                            );
                            return;
                        }

                        // Clone to avoid borrowing issues
                        let variant_types_cloned = variant_types.clone();

                        // Build generic mapping from subject_type's generic args
                        let generic_mapping: HashMap<String, Type> =
                            if let TypeKind::Custom(_, Some(ref args)) = &subject_type.kind {
                                if let Some(ref generics) = enum_def.generics {
                                    generics
                                        .iter()
                                        .zip(args.iter())
                                        .filter_map(|(g, arg_expr)| {
                                            self.extract_type_from_expression(arg_expr)
                                                .ok()
                                                .map(|ty| (g.name.clone(), ty))
                                        })
                                        .collect()
                                } else {
                                    HashMap::new()
                                }
                            } else {
                                HashMap::new()
                            };

                        // Bind each pattern with its type (substituting generics if needed)
                        for (binding, var_type) in bindings.iter().zip(variant_types_cloned.iter())
                        {
                            let resolved_type = if generic_mapping.is_empty() {
                                var_type.clone()
                            } else {
                                self.substitute_type(var_type, &generic_mapping)
                            };
                            self.check_pattern(binding, &resolved_type, context, span, is_mutable);
                        }

                        // Check if subject type matches the enum type
                        // Preserve generic args from subject_type
                        let generic_args =
                            if let TypeKind::Custom(sub_name, ref sub_args) = &subject_type.kind {
                                if sub_name == &enum_name {
                                    sub_args.clone()
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                        let expected_type =
                            make_type(TypeKind::Custom(enum_name.clone(), generic_args));
                        if !self.are_compatible(subject_type, &expected_type, context) {
                            self.report_error(
                                format!(
                                    "Pattern type mismatch: expected {}, got {}",
                                    subject_type, expected_type
                                ),
                                span,
                            );
                        }
                    } else {
                        self.report_error(
                            format!("Enum '{}' has no variant '{}'", enum_name, variant_name),
                            span,
                        );
                    }
                } else {
                    self.report_error(format!("'{}' is not an Enum", enum_name), span);
                }
            }
        }
    }
}
