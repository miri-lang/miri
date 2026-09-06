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
use crate::ast::types::{Type, TypeKind, OPTION_TYPE_NAME};
use crate::ast::*;
use crate::diagnostics::repair::{RepairRequest, VariantPatternSite};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::type_checker::context::{Context, SymbolInfo, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

/// The enum facts an exhaustiveness check consults, detached from the definition.
struct EnumMatchFacts {
    remaining_variants: HashSet<String>,
    module: String,
    non_exhaustive: bool,
}

impl TypeChecker {
    /// Returns `true` if the given expression is an assignment of any form.
    ///
    /// Detects all assignment operators: `=`, `+=`, `-=`, `*=`, `/=`, `%=`.
    fn is_assignment_expression(&self, expr: &Expression) -> bool {
        matches!(expr.node, ExpressionKind::Assignment(_, _, _))
    }

    /// Returns `true` if the given arm body produces a value of its own.
    ///
    /// An assignment performs a side effect; it does not produce a value the
    /// surrounding construct can hand back. Such an arm therefore constrains
    /// nothing about what the other arms produce.
    fn arm_produces_value(&self, body: &Statement) -> bool {
        match &body.node {
            StatementKind::Expression(expr) => !self.is_assignment_expression(expr),
            _ => true,
        }
    }

    /// Checks type agreement across non-void arms and returns the agreed type.
    ///
    /// Applies the Void-skip rule (if all arms are void, returns Void) and the
    /// "first non-void arm seeds the type" rule. Reports mismatches at the
    /// offending arm's span with structured expected/actual types.
    fn check_branch_agreement(
        &mut self,
        arm_types: &[(Type, Span)],
        format_message: impl Fn(&str, &str) -> String,
        context: &mut Context,
    ) -> Type {
        let mut first_branch_type = None;
        for (body_type, arm_span) in arm_types {
            if matches!(body_type.kind, TypeKind::Void) {
                continue;
            }

            if let Some(first) = &first_branch_type {
                if !self.are_compatible(first, body_type, context) {
                    let message = format_message(&first.to_string(), &body_type.to_string());
                    self.report_error_with_types(
                        DiagnosticCode::TypTypeMismatch,
                        message,
                        *arm_span,
                        first.to_string(),
                        body_type.to_string(),
                    );
                }
            } else {
                first_branch_type = Some(body_type.clone());
            }
        }

        first_branch_type.unwrap_or(make_type(TypeKind::Void))
    }

    pub(crate) fn infer_match(
        &mut self,
        subject: &Expression,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let subject_type = self.infer_expression(subject, context);

        // A pattern that named a constructor without its enum never resolved,
        // so the match covers no variant at all. Reporting it non-exhaustive on
        // top of that would tell the author to add an arm they already wrote,
        // and every name those patterns bind would be reported undefined in the
        // arm bodies. One report for one mistake: the prefix is missing.
        let unresolved = unresolved_variant_patterns(&subject_type, branches);
        if unresolved.is_empty() {
            // Exhaustiveness is a property of the subject's variant set, so the
            // report points at the subject rather than at the `match` keyword:
            // that is the expression whose type the author has to cover.
            self.check_exhaustiveness_enum(&subject_type, branches, subject.span, context);
            self.check_exhaustiveness_option(&subject_type, branches, subject.span, context);
        } else {
            self.report_unresolved_variant_patterns(&subject_type, &unresolved);
        }

        if branches.is_empty() {
            return make_type(TypeKind::Void);
        }

        self.infer_match_body_type(&subject_type, branches, span, context)
    }

    /// Reports every pattern that named a variant constructor without its enum
    /// as one diagnostic.
    ///
    /// The first is the primary; the rest hang off it as related notes carrying
    /// their own location, because they are the same mistake repeated and one
    /// repair covers them all.
    fn report_unresolved_variant_patterns(
        &mut self,
        subject_type: &Type,
        unresolved: &[UnresolvedVariantPattern],
    ) {
        let Some((primary, echoes)) = unresolved.split_first() else {
            return;
        };
        let qualified = |pattern: &UnresolvedVariantPattern| {
            format!(
                "Expected enum variant pattern like {}.{}",
                subject_enum_name(subject_type).unwrap_or("EnumName"),
                pattern.variant
            )
        };
        let related = echoes
            .iter()
            .map(|echo| RelatedNote::at(qualified(echo), echo.span))
            .collect();
        let help = subject_enum_name(subject_type).map(|enum_name| {
            format!(
                "write '{}.{}' so the pattern names the variant it matches; a bare \
                 name binds a new variable instead.",
                enum_name, primary.variant
            )
        });
        let repair =
            subject_enum_name(subject_type).map(|enum_name| RepairRequest::QualifyVariantPattern {
                enum_name: enum_name.to_string(),
                sites: unresolved
                    .iter()
                    .map(|pattern| VariantPatternSite {
                        start: pattern.span.start,
                        variant: pattern.variant.clone(),
                    })
                    .collect(),
            });
        self.report_error_with_related(
            DiagnosticCode::TypEnumVariant,
            qualified(primary),
            primary.span,
            help,
            related,
            repair,
        );
    }

    /// Checks exhaustiveness for enum types in match expressions.
    fn check_exhaustiveness_enum(
        &mut self,
        subject_type: &Type,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) {
        let TypeKind::Custom(name, _) = &subject_type.kind else {
            return;
        };
        let Some(EnumMatchFacts {
            mut remaining_variants,
            module,
            non_exhaustive,
        }) = self.find_enum_match_facts(name, context)
        else {
            return;
        };

        let has_catch_all =
            self.extract_covered_enum_variants(name, branches, &mut remaining_variants);

        // An open variant set makes listing today's variants insufficient: only a
        // catch-all keeps the match compiling when a variant is added later. Inside
        // the defining module the full set is enforced, so its own matches are
        // updated alongside the new variant.
        //
        // `module` is stamped once where the enum is declared and never rewritten,
        // so re-exporting or transitively importing the enum still compares against
        // the module that owns the variants.
        if non_exhaustive && module != self.modules.current_module {
            if !has_catch_all {
                self.report_typed_error(TypeError::new(
                    TypeErrorKind::NonExhaustiveEnumNeedsCatchAll {
                        enum_name: name.clone(),
                        module,
                    },
                    span,
                ));
            }
            return;
        }

        if !has_catch_all && !remaining_variants.is_empty() {
            let mut missing: Vec<_> = remaining_variants.into_iter().collect();
            missing.sort();
            self.report_error(
                DiagnosticCode::TypEnumVariant,
                format!(
                    "Non-exhaustive match on Enum '{}'. Missing variants: {}",
                    name,
                    missing.join(", ")
                ),
                span,
            );
        }
    }

    /// Everything exhaustiveness checking needs from an enum definition, owned so
    /// the borrow of the definition ends before diagnostics are reported. Only the
    /// variant names are copied; the generics and method tables are left untouched.
    fn find_enum_match_facts(&self, name: &str, context: &Context) -> Option<EnumMatchFacts> {
        let definition = context
            .type_definitions
            .iter()
            .rev()
            .find_map(|scope| match scope.get(name) {
                Some(TypeDefinition::Enum(def)) => Some(def),
                _ => None,
            })
            .or_else(|| match self.type_table.global_type_definitions.get(name) {
                Some(TypeDefinition::Enum(def)) => Some(def),
                _ => None,
            })?;

        Some(EnumMatchFacts {
            remaining_variants: definition.variants.keys().cloned().collect(),
            module: definition.module.clone(),
            non_exhaustive: definition.non_exhaustive,
        })
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
                DiagnosticCode::TypEnumVariant,
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
                                if parent_name == OPTION_TYPE_NAME {
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
                                    if name == OPTION_TYPE_NAME && variant == "Some" {
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
    ///
    /// Only when all arms are assignments or void does the match type as Void
    /// (no agreement check). When at least one arm produces a genuine value,
    /// agreement is enforced across all non-void arms (including assignments),
    /// because assignment arms materialize their value into the shared result slot.
    fn infer_match_body_type(
        &mut self,
        subject_type: &Type,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let mut arm_types = Vec::new();
        let mut has_genuine_value = false;

        for branch in branches {
            context.enter_scope();
            // A pattern diagnostic points at the pattern the author wrote, not
            // at the `match` that holds it; the whole-match span stands in only
            // for a branch built programmatically, which has no source text.
            for (index, pattern) in branch.patterns.iter().enumerate() {
                let pattern_span = branch.pattern_span(index).unwrap_or(span);
                self.check_pattern(
                    pattern,
                    subject_type,
                    context,
                    pattern_span,
                    branch.is_mutable,
                );
            }

            let body_type = self.infer_statement_type(&branch.body, context);
            let arm_span = self.get_body_expression_span(&branch.body);
            context.exit_scope();

            has_genuine_value |=
                self.arm_produces_value(&branch.body) && !matches!(body_type.kind, TypeKind::Void);
            arm_types.push((body_type, arm_span));
        }

        if !has_genuine_value {
            return make_type(TypeKind::Void);
        }

        self.check_branch_agreement(
            &arm_types,
            |expected, actual| {
                format!(
                    "Match branch types mismatch: expected {}, got {}",
                    expected, actual
                )
            },
            context,
        )
    }

    /// Returns the span of the expression inside a statement body.
    ///
    /// For statement kinds that are expressions (like an assignment or function call),
    /// returns the expression's span. For other statement kinds, returns the statement's span.
    fn get_body_expression_span(&self, body: &Statement) -> Span {
        match &body.node {
            StatementKind::Expression(expr) => expr.span,
            _ => body.span,
        }
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
                DiagnosticCode::TypTypeMismatch,
                format!("Conditional condition must be a boolean, got {}", cond_type),
                cond_expr.span,
            );
        }

        let then_type = self.infer_expression(then_expr, context);

        if let Some(else_expr) = else_expr_opt {
            let else_type = self.infer_expression(else_expr, context);

            let then_is_assignment = self.is_assignment_expression(then_expr);
            let else_is_assignment = self.is_assignment_expression(else_expr);
            let then_is_void = matches!(then_type.kind, TypeKind::Void);
            let else_is_void = matches!(else_type.kind, TypeKind::Void);

            let has_genuine_value =
                (!then_is_assignment && !then_is_void) || (!else_is_assignment && !else_is_void);

            if !has_genuine_value {
                return make_type(TypeKind::Void);
            }

            let branch_types = [(then_type, then_expr.span), (else_type, else_expr.span)];

            self.check_branch_agreement(
                &branch_types,
                |expected, actual| {
                    format!(
                        "Conditional branches must have the same type: expected {}, got {}",
                        expected, actual
                    )
                },
                context,
            )
        } else {
            if !self.are_compatible(&then_type, &make_type(TypeKind::Void), context) {
                self.report_error(
                    DiagnosticCode::TypPatternMatch,
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
        let lit_type = self.infer_literal(lit, context);
        if !self.are_compatible(subject_type, &lit_type, context) {
            self.report_error(
                DiagnosticCode::TypPatternMatch,
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
                self.modules.current_module.clone(),
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
                    DiagnosticCode::TypPatternMatch,
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
                DiagnosticCode::TypPatternMatch,
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
            if parent_name == OPTION_TYPE_NAME
                && member == "None"
                && matches!(subject_type.kind, TypeKind::Option(_))
            {
                return;
            }

            let enum_def_opt = self.resolve_visible_type(parent_name, context).cloned();
            if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                if !enum_def.variants.contains_key(member) {
                    self.report_error(
                        DiagnosticCode::TypEnumVariant,
                        format!("Enum '{}' has no variant '{}'", parent_name, member),
                        span,
                    );
                }
                let expected_type = self.build_enum_member_type(parent_name, subject_type);
                if !self.are_compatible(subject_type, &expected_type, context) {
                    self.report_error(
                        DiagnosticCode::TypPatternMatch,
                        format!(
                            "Pattern type mismatch: expected {}, got {}",
                            subject_type, expected_type
                        ),
                        span,
                    );
                }
            } else {
                self.report_error(
                    DiagnosticCode::TypEnumDefinition,
                    format!("'{}' is not an Enum", parent_name),
                    span,
                );
            }
        } else {
            self.report_error(
                DiagnosticCode::TypPatternMatch,
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
                DiagnosticCode::TypPatternMatch,
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
            None => {
                // A subject that names no enum is not covered by the
                // whole-match pass, so the report is raised here instead.
                if subject_enum_name(subject_type).is_none() {
                    self.report_error(
                        DiagnosticCode::TypEnumVariant,
                        format!(
                            "Expected an enum variant pattern, but the subject has type {}",
                            subject_type
                        ),
                        span,
                    );
                }
                // The pattern named no enum, so nothing can be resolved from
                // it. Its bindings are still defined, at the error type, so the
                // arm body is not reported against the enclosing scope for
                // names the author did write.
                self.bind_unresolved_pattern_names(bindings, is_mutable, context);
                return;
            }
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
                    name == OPTION_TYPE_NAME && variant == "Some"
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
                DiagnosticCode::TypPatternMatch,
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

    /// Defines the names `bindings` introduces at the error type.
    ///
    /// A pattern that failed to resolve binds nothing, and an arm body that
    /// then mentions those names reports each as an undefined variable — a
    /// cascade of the one mistake, complete with a `Did you mean` line drawn
    /// from a scope the pattern never reached. Binding them here keeps the
    /// report to the pattern itself; the error type suppresses what follows.
    fn bind_unresolved_pattern_names(
        &mut self,
        bindings: &[Pattern],
        is_mutable: bool,
        context: &mut Context,
    ) {
        for binding in bindings {
            match binding {
                Pattern::Identifier(name) => {
                    self.check_pattern_identifier(
                        name,
                        &make_type(TypeKind::Error),
                        is_mutable,
                        context,
                    );
                }
                Pattern::EnumVariant(_, nested) | Pattern::Tuple(nested) => {
                    self.bind_unresolved_pattern_names(nested, is_mutable, context);
                }
                Pattern::Literal(_) | Pattern::Member(_, _) | Pattern::Regex(_) => {}
                Pattern::Default => {}
            }
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
                        DiagnosticCode::TypPatternMatch,
                        "Complex member patterns are not supported".to_string(),
                        span,
                    );
                    None
                }
            }
            // A bare constructor name over an enum subject is reported once for
            // the whole match, by `report_unresolved_variant_patterns`, so that
            // repeating the mistake on several arms stays one diagnostic.
            Pattern::Identifier(_) => None,
            _ => {
                self.report_error(
                    DiagnosticCode::TypEnumVariant,
                    "Invalid enum variant pattern".to_string(),
                    span,
                );
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
                        DiagnosticCode::TypEnumVariant,
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
                        DiagnosticCode::TypPatternMatch,
                        format!(
                            "Pattern type mismatch: expected {}, got {}",
                            subject_type, expected_type
                        ),
                        span,
                    );
                }
            } else {
                self.report_error(
                    DiagnosticCode::TypEnumVariant,
                    format!("Enum '{}' has no variant '{}'", enum_name, variant_name),
                    span,
                );
            }
        } else {
            self.report_error(
                DiagnosticCode::TypEnumDefinition,
                format!("'{}' is not an Enum", enum_name),
                span,
            );
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

/// A pattern that named a variant constructor without the enum that owns it,
/// such as `Ok(n)` where `Result.Ok(n)` is required.
struct UnresolvedVariantPattern {
    variant: String,
    span: Span,
}

/// The enum a match subject names, when it names one.
fn subject_enum_name(subject_type: &Type) -> Option<&str> {
    let TypeKind::Custom(name, _) = &subject_type.kind else {
        return None;
    };
    Some(name)
}

/// Every pattern across `branches` that carries bindings but names its variant
/// without an enum.
///
/// `Some(v)` and `None` over a `T?` subject are spelled bare on purpose, so a
/// match on an optional yields nothing here.
fn unresolved_variant_patterns(
    subject_type: &Type,
    branches: &[MatchBranch],
) -> Vec<UnresolvedVariantPattern> {
    // `Option` reaches here spelled either way — as the sugar `T?` or as the
    // named type — and its constructors are written bare in both.
    let Some(enum_name) = subject_enum_name(subject_type) else {
        return Vec::new();
    };
    if enum_name == OPTION_TYPE_NAME {
        return Vec::new();
    }
    let mut unresolved = Vec::new();
    for branch in branches {
        for (index, pattern) in branch.patterns.iter().enumerate() {
            let Pattern::EnumVariant(parent, _) = pattern else {
                continue;
            };
            let Pattern::Identifier(variant) = &**parent else {
                continue;
            };
            let Some(span) = branch.pattern_span(index) else {
                continue;
            };
            unresolved.push(UnresolvedVariantPattern {
                variant: variant.clone(),
                span,
            });
        }
    }
    unresolved
}
