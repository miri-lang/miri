// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names a match arm's patterns bind.
//!
//! An arm written as alternatives, `A(x) | B(x)`, runs one body whichever
//! alternative matched, so every name the body can read has to be bound by
//! every alternative, and at one type. A name only some alternatives bind
//! would be read uninitialized on the others; a name bound at two types would
//! be read at the wrong one.

use std::collections::HashMap;

use crate::ast::pattern::MatchBranch;
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::TypeChecker;

/// The names one pattern binds, with what each is bound as.
type Bindings = HashMap<String, SymbolInfo>;

impl TypeChecker {
    /// Check each of `branch`'s patterns against the subject and bind the
    /// names they introduce in the current scope.
    ///
    /// Alternatives are checked one at a time, each in a scope of its own, so
    /// what one binds can be compared with what the others bind before the
    /// arm's body sees any of it.
    pub(crate) fn check_arm_patterns(
        &mut self,
        branch: &MatchBranch,
        subject_type: &Type,
        match_span: Span,
        context: &mut Context,
    ) {
        let mut arm_bindings: Option<Bindings> = None;
        for (index, pattern) in branch.patterns.iter().enumerate() {
            // A pattern diagnostic points at the pattern the author wrote, not
            // at the `match` that holds it; the whole-match span stands in only
            // for a branch built programmatically, which has no source text.
            let pattern_span = branch.pattern_span(index).unwrap_or(match_span);
            context.enter_scope();
            self.check_pattern(
                pattern,
                subject_type,
                context,
                pattern_span,
                branch.is_mutable,
            );
            let bindings = context.scopes.last_mut().map(std::mem::take);
            context.exit_scope();
            let bindings = bindings.unwrap_or_default();
            match &arm_bindings {
                None => arm_bindings = Some(bindings),
                Some(first) => {
                    self.check_alternative_bindings(first, &bindings, pattern_span, context)
                }
            }
        }
        for (name, info) in arm_bindings.unwrap_or_default() {
            context.define(name, info);
        }
    }

    /// Report each name bound by only one of `first` and `other`, and each
    /// name the two bind at different types.
    fn check_alternative_bindings(
        &mut self,
        first: &Bindings,
        other: &Bindings,
        span: Span,
        context: &Context,
    ) {
        // `_` discards the value it stands for, so it binds nothing to agree on.
        let mut names: Vec<&String> = first
            .keys()
            .chain(other.keys())
            .filter(|name| name.as_str() != "_")
            .collect();
        names.sort();
        names.dedup();
        for name in names {
            match (first.get(name), other.get(name)) {
                (Some(a), Some(b)) => {
                    self.check_alternative_binding_type(name, a, b, span, context)
                }
                (Some(_), None) | (None, Some(_)) => self.report_error(
                    DiagnosticCode::TypPatternMatch,
                    format!(
                        "'{name}' is bound in some alternatives of this arm but not in all of them"
                    ),
                    span,
                ),
                (None, None) => {}
            }
        }
    }

    /// Report `name` when the two alternatives bind it at different types.
    fn check_alternative_binding_type(
        &mut self,
        name: &str,
        first: &SymbolInfo,
        other: &SymbolInfo,
        span: Span,
        context: &Context,
    ) {
        let is_unresolved = |info: &SymbolInfo| matches!(info.ty.kind, TypeKind::Error);
        if is_unresolved(first) || is_unresolved(other) {
            return;
        }
        let agree = self.are_compatible(&first.ty, &other.ty, context)
            && self.are_compatible(&other.ty, &first.ty, context);
        if !agree {
            self.report_error(
                DiagnosticCode::TypPatternMatch,
                format!(
                    "'{name}' is bound at {} in one alternative of this arm and at {} in another",
                    first.ty, other.ty
                ),
                span,
            );
        }
    }
}
