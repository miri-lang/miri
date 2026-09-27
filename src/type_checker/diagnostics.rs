// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic collection for type checking results.
//!
//! This module provides the [`DiagnosticCollector`] struct, which aggregates
//! type errors, warnings, and deduplication state for a single type-checking pass.

use crate::error::diagnostic::Diagnostic;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use std::collections::HashSet;

/// Collects type checking diagnostics: errors, warnings, and reported error deduplication.
#[derive(Debug, Clone)]
pub struct DiagnosticCollector {
    /// Type errors encountered during checking.
    pub errors: Vec<TypeError>,
    /// Type warnings encountered during checking.
    pub warnings: Vec<Diagnostic>,
    /// Deduplication set for (message, span) pairs to avoid duplicate error reports.
    pub(crate) reported_errors: HashSet<(String, Span)>,
}

impl Default for DiagnosticCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticCollector {
    /// Creates a new empty diagnostic collector.
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            reported_errors: HashSet::new(),
        }
    }

    /// Adds a type error to the collection.
    pub(crate) fn push_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// Adds a type warning to the collection.
    pub(crate) fn push_warning(&mut self, warning: Diagnostic) {
        self.warnings.push(warning);
    }

    /// Extends the error list with errors from another collection.
    pub(crate) fn extend_errors(&mut self, errors: Vec<TypeError>) {
        self.errors.extend(errors);
    }

    /// Extends the warning list with warnings from another collection.
    pub(crate) fn extend_warnings(&mut self, warnings: Vec<Diagnostic>) {
        self.warnings.extend(warnings);
    }

    /// Returns `true` if there are no errors.
    pub(crate) fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Clones the error list.
    pub(crate) fn clone_errors(&self) -> Vec<TypeError> {
        self.errors.clone()
    }

    /// Records that an error with the given (message, span) has been reported.
    /// Returns `true` if this is the first time this error is being reported,
    /// `false` if it was already reported (deduplication).
    pub(crate) fn mark_reported(&mut self, key: (String, Span)) -> bool {
        self.reported_errors.insert(key)
    }
}

/// `ty` as a diagnostic spells it: a collection reads back as the source
/// writes it, `List<int?>`, at every depth, rather than as the internal
/// `List(int?)` the type's own display gives. Every other type displays as
/// it does elsewhere.
pub(crate) fn spelled(ty: &crate::ast::types::Type) -> String {
    use crate::ast::types::TypeKind;
    let mut normalized = ty.clone();
    crate::ast::normalize::normalize_type(&mut normalized);
    if let TypeKind::Option(inner) = &normalized.kind {
        return format!("{}?", spelled(inner));
    }
    if let TypeKind::Function(function) = &normalized.kind {
        return spelled_function(function);
    }
    if crate::type_checker::generics::extract_value_generic(&normalized).is_some() {
        return normalized.to_string();
    }
    let TypeKind::Custom(name, Some(arguments)) = &normalized.kind else {
        return normalized.to_string();
    };
    let arguments: Vec<String> = arguments.iter().map(spelled_argument).collect();
    format!("{name}<{}>", arguments.join(", "))
}

/// A written type argument as a diagnostic spells it: a type through
/// [`spelled`], a value as written.
fn spelled_argument(argument: &crate::ast::Expression) -> String {
    if let crate::ast::ExpressionKind::Type(ty, nullable) = &argument.node {
        let written = spelled(ty);
        return if *nullable {
            format!("{written}?")
        } else {
            written
        };
    }
    argument.node.to_string()
}

/// A function type as a diagnostic spells it, with each parameter and the
/// result through [`spelled_argument`], so a nullable result keeps its `?`.
fn spelled_function(function: &crate::ast::types::FunctionTypeData) -> String {
    let parameters: Vec<String> = function
        .params
        .iter()
        .map(|parameter| spelled_argument(&parameter.typ))
        .collect();
    let result = function
        .return_type
        .as_deref()
        .map(|ret| format!(" -> {}", spelled_argument(ret)))
        .unwrap_or_default();
    format!("Function({}){result}", parameters.join(", "))
}
