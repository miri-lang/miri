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

        self.check_exhaustiveness_enum(&subject_type, branches, span, context);
        self.check_exhaustiveness_option(&subject_type, branches, span, context);

        if branches.is_empty() {
            return make_type(TypeKind::Void);
        }

        self.infer_match_body_type(&subject_type, branches, span, context)
    }

    /// Checks exhaustiveness for enum types in match expressions.
    fn check_exhaustiveness_enum(
        &mut self,
        subject_type: &Type,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) {
        if let TypeKind::Custom(name, _) = &subject_type.kind {
            let enum_def_opt = self.find_enum_definition(name, context);
            if let Some(enum_def) = enum_def_opt {
                let mut remaining_variants: HashSet<String> = enum_def.keys().cloned().collect();
                let is_exhaustive =
                    self.extract_covered_enum_variants(name, branches, &mut remaining_variants);

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
    }

    /// Finds an enum definition by name, checking local and global scopes.
    fn find_enum_definition(
        &self,
        name: &str,
        context: &Context,
    ) -> Option<std::collections::BTreeMap<String, Vec<Type>>> {
        for scope in context.type_definitions.iter().rev() {
            if let Some(TypeDefinition::Enum(def)) = scope.get(name) {
                return Some(def.variants.clone());
            }
        }
        if let Some(TypeDefinition::Enum(def)) = self.global_type_definitions.get(name) {
            return Some(def.variants.clone());
        }
        None
    }

    /// Extracts covered enum variants from match branches and checks exhaustiveness.
    fn extract_covered_enum_variants(
        &self,
        enum_name: &str,
        branches: &[MatchBranch],
        remaining_variants: &mut HashSet<String>,
    ) -> bool {
        let mut is_exhaustive = false;
        for branch in branches {
            if branch.guard.is_none() {
                for pattern in &branch.patterns {
                    match pattern {
                        Pattern::Default | Pattern::Identifier(_) => {
                            is_exhaustive = true;
                        }
                        Pattern::Member(parent, member) => {
                            if let Pattern::Identifier(parent_name) = &**parent {
                                if parent_name == enum_name {
                                    remaining_variants.remove(member);
                                }
                            }
                        }
                        Pattern::EnumVariant(parent, _) => {
                            if let Pattern::Member(enum_name_pat, variant_name) = &**parent {
                                if let Pattern::Identifier(enum_name_str) = &**enum_name_pat {
                                    if enum_name_str == enum_name {
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
        is_exhaustive
    }

    /// Checks exhaustiveness for Option types in match expressions.
    fn check_exhaustiveness_option(
        &mut self,
        subject_type: &Type,
        branches: &[MatchBranch],
        span: Span,
        _context: &Context,
    ) {
        if !matches!(subject_type.kind, TypeKind::Option(_)) {
            return;
        }

        let (has_some, has_none, is_exhaustive) = self.extract_option_coverage(branches);

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

    /// Extracts coverage information for Option variants from match branches.
    fn extract_option_coverage(&self, branches: &[MatchBranch]) -> (bool, bool, bool) {
        let mut has_some = false;
        let mut has_none = false;
        let mut is_exhaustive = false;

        for branch in branches {
            if branch.guard.is_none() {
                for pattern in &branch.patterns {
                    match pattern {
                        Pattern::Default | Pattern::Identifier(_) => {
                            is_exhaustive = true;
                        }
                        Pattern::Literal(crate::ast::literal::Literal::None) => {
                            has_none = true;
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
        (has_some, has_none, is_exhaustive)
    }

    /// Infers the result type from match branches, checking type compatibility.
    fn infer_match_body_type(
        &mut self,
        subject_type: &Type,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let mut first_branch_type = None;

        for branch in branches {
            context.enter_scope();
            for pattern in &branch.patterns {
                self.check_pattern(pattern, subject_type, context, span, branch.is_mutable);
            }

            let body_type = self.infer_statement_type(&branch.body, context);
            context.exit_scope();

            if matches!(body_type.kind, TypeKind::Void) {
                continue;
            }

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
        if !matches!(cond_type.kind, TypeKind::Boolean | TypeKind::Error) {
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
                self.check_pattern_literal(lit, subject_type, span, context);
            }
            Pattern::Identifier(name) => {
                self.check_pattern_identifier(name, subject_type, is_mutable, context);
            }
            Pattern::Tuple(patterns) => {
                self.check_pattern_tuple(patterns, subject_type, span, is_mutable, context);
            }
            Pattern::Member(parent, member) => {
                self.check_pattern_member(parent, member, subject_type, span, context);
            }
            Pattern::Regex(_) => {
                self.check_pattern_regex(subject_type, span);
            }
            Pattern::Default => {}
            Pattern::EnumVariant(parent_pattern, bindings) => {
                self.check_pattern_enum_variant(
                    parent_pattern,
                    bindings,
                    subject_type,
                    span,
                    is_mutable,
                    context,
                );
            }
        }
    }

    /// Validates a literal pattern against the subject type.
    fn check_pattern_literal(
        &mut self,
        lit: &crate::ast::literal::Literal,
        subject_type: &Type,
        span: Span,
        context: &mut Context,
    ) {
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

    /// Validates and binds an identifier pattern.
    fn check_pattern_identifier(
        &mut self,
        name: &str,
        subject_type: &Type,
        is_mutable: bool,
        context: &mut Context,
    ) {
        context.define(
            name.to_string(),
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

    /// Validates a tuple pattern with element count and type checking.
    fn check_pattern_tuple(
        &mut self,
        patterns: &[Pattern],
        subject_type: &Type,
        span: Span,
        is_mutable: bool,
        context: &mut Context,
    ) {
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

            let elem_types_cloned = elem_types.clone();
            for (i, pat) in patterns.iter().enumerate() {
                let elem_type = self.resolve_type_expression(&elem_types_cloned[i], context);
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

    /// Validates a member pattern (e.g., `Option.None`).
    fn check_pattern_member(
        &mut self,
        parent: &Pattern,
        member: &str,
        subject_type: &Type,
        span: Span,
        context: &mut Context,
    ) {
        if let Pattern::Identifier(parent_name) = parent {
            if parent_name == "Option"
                && member == "None"
                && matches!(subject_type.kind, TypeKind::Option(_))
            {
                return;
            }

            let enum_def_opt = self.resolve_visible_type(parent_name, context).cloned();
            if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                if !enum_def.variants.contains_key(member) {
                    self.report_error(
                        format!("Enum '{}' has no variant '{}'", parent_name, member),
                        span,
                    );
                }
                let expected_type = self.build_enum_member_type(parent_name, subject_type);
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

    /// Builds the expected enum type for a member pattern.
    fn build_enum_member_type(&self, enum_name: &str, subject_type: &Type) -> Type {
        if let TypeKind::Custom(sub_name, sub_args) = &subject_type.kind {
            if sub_name == enum_name {
                return make_type(TypeKind::Custom(enum_name.to_string(), sub_args.clone()));
            }
        }
        make_type(TypeKind::Custom(enum_name.to_string(), None))
    }

    /// Validates a regex pattern requires string subject.
    fn check_pattern_regex(&mut self, subject_type: &Type, span: Span) {
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

    /// Validates an enum variant pattern with bindings.
    fn check_pattern_enum_variant(
        &mut self,
        parent_pattern: &Pattern,
        bindings: &[Pattern],
        subject_type: &Type,
        span: Span,
        is_mutable: bool,
        context: &mut Context,
    ) {
        if matches!(subject_type.kind, TypeKind::Option(_))
            && self.is_option_some_pattern(parent_pattern)
        {
            self.check_pattern_option_some(bindings, subject_type, span, is_mutable, context);
            return;
        }

        let (enum_name, variant_name) = match self.extract_enum_variant_name(parent_pattern, span) {
            Some((e, v)) => (e, v),
            None => return,
        };

        self.check_enum_variant_bindings(
            &enum_name,
            &variant_name,
            bindings,
            subject_type,
            span,
            is_mutable,
            context,
        );
    }

    /// Checks if a pattern is an `Option.Some` variant.
    fn is_option_some_pattern(&self, parent_pattern: &Pattern) -> bool {
        match parent_pattern {
            Pattern::Identifier(name) => name == "Some",
            Pattern::Member(enum_pat, variant) => {
                if let Pattern::Identifier(name) = &**enum_pat {
                    name == "Option" && variant == "Some"
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Checks the bindings for an `Option.Some(x)` pattern.
    fn check_pattern_option_some(
        &mut self,
        bindings: &[Pattern],
        subject_type: &Type,
        span: Span,
        is_mutable: bool,
        context: &mut Context,
    ) {
        if bindings.len() != 1 {
            self.report_error(
                format!("Some pattern expects 1 binding, got {}", bindings.len()),
                span,
            );
            return;
        }
        if let TypeKind::Option(inner) = &subject_type.kind {
            let inner_type = inner.as_ref().clone();
            self.check_pattern(&bindings[0], &inner_type, context, span, is_mutable);
        }
    }

    /// Extracts enum and variant names from a parent pattern.
    fn extract_enum_variant_name(
        &mut self,
        parent_pattern: &Pattern,
        span: Span,
    ) -> Option<(String, String)> {
        match parent_pattern {
            Pattern::Member(enum_pat, variant) => {
                if let Pattern::Identifier(name) = &**enum_pat {
                    Some((name.clone(), variant.clone()))
                } else {
                    self.report_error(
                        "Complex member patterns are not supported".to_string(),
                        span,
                    );
                    None
                }
            }
            Pattern::Identifier(name) => {
                self.report_error(
                    format!("Expected enum variant pattern like EnumName.{}", name),
                    span,
                );
                None
            }
            _ => {
                self.report_error("Invalid enum variant pattern".to_string(), span);
                None
            }
        }
    }

    /// Validates enum variant bindings and checks type compatibility.
    #[allow(clippy::too_many_arguments)]
    fn check_enum_variant_bindings(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        bindings: &[Pattern],
        subject_type: &Type,
        span: Span,
        is_mutable: bool,
        context: &mut Context,
    ) {
        let enum_def_opt = self.resolve_visible_type(enum_name, context).cloned();
        if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
            if let Some(variant_types) = enum_def.variants.get(variant_name) {
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

                let variant_types_cloned = variant_types.clone();
                let generic_mapping =
                    self.build_generic_mapping(enum_name, subject_type, &enum_def);

                for (binding, var_type) in bindings.iter().zip(variant_types_cloned.iter()) {
                    let resolved_type = if generic_mapping.is_empty() {
                        var_type.clone()
                    } else {
                        self.substitute_type(var_type, &generic_mapping)
                    };
                    self.check_pattern(binding, &resolved_type, context, span, is_mutable);
                }

                let expected_type = self.build_enum_variant_type(enum_name, subject_type);
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

    /// Builds a generic type mapping from subject type and enum definition.
    fn build_generic_mapping(
        &mut self,
        _enum_name: &str,
        subject_type: &Type,
        enum_def: &crate::type_checker::context::EnumDefinition,
    ) -> HashMap<String, Type> {
        if let TypeKind::Custom(_, Some(ref args)) = &subject_type.kind {
            if let Some(ref generics) = enum_def.generics {
                return generics
                    .iter()
                    .zip(args.iter())
                    .filter_map(|(g, arg_expr)| {
                        self.extract_type_from_expression(arg_expr)
                            .ok()
                            .map(|ty| (g.name.clone(), ty))
                    })
                    .collect();
            }
        }
        HashMap::new()
    }

    /// Builds the expected enum type for a variant pattern.
    fn build_enum_variant_type(&self, enum_name: &str, subject_type: &Type) -> Type {
        let generic_args = if let TypeKind::Custom(sub_name, ref sub_args) = &subject_type.kind {
            if sub_name == enum_name {
                sub_args.clone()
            } else {
                None
            }
        } else {
            None
        };
        make_type(TypeKind::Custom(enum_name.to_string(), generic_args))
    }
}
