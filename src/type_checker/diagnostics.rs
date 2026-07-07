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
