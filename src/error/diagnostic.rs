// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Core diagnostic types for unified error and warning reporting.
//!
//! This module provides the foundational types for the error infrastructure:
//! - [`Severity`] - Error, Warning, or Note level
//! - [`Diagnostic`] - Rich diagnostic message with all context
//! - [`Reportable`] - Trait for types that can produce diagnostics

use crate::error::syntax::Span;

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Hard error - compilation stops.
    Error,
    /// Warning - compilation continues, user should address.
    Warning,
    /// Note - additional context for another diagnostic.
    Note,
}

impl Severity {
    /// Get the display name for this severity level.
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A rich, user-facing diagnostic message.
///
/// Diagnostics provide all the context needed to display helpful error messages:
/// - Severity level (error, warning, note)
/// - Optional error code for documentation/tooling
/// - Human-readable title and detailed message
/// - Source location (span)
/// - Actionable help text
/// - Additional notes for multi-context errors
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity level (error, warning, note).
    pub severity: Severity,
    /// Error code for documentation/tooling (e.g., "E0001", "W0012").
    pub code: Option<&'static str>,
    /// Short, human-readable title (e.g., "Undefined Variable").
    pub title: String,
    /// Detailed explanation message.
    pub message: String,
    /// Source span where the issue occurred.
    pub span: Option<Span>,
    /// Actionable help text.
    pub help: Option<String>,
    /// Additional notes/context.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Create a new error diagnostic.
    pub fn error(title: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(Severity::Error, title)
    }

    /// Create a new warning diagnostic.
    pub fn warning(title: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(Severity::Warning, title)
    }

    /// Create a new note diagnostic.
    pub fn note(title: impl Into<String>) -> DiagnosticBuilder {
        DiagnosticBuilder::new(Severity::Note, title)
    }

    /// Format this diagnostic for terminal output.
    pub fn format(&self, source: &str) -> String {
        use crate::error::format::format_diagnostic_full;
        format_diagnostic_full(source, self)
    }
}

/// Builder for constructing diagnostics ergonomically.
#[derive(Debug)]
pub struct DiagnosticBuilder {
    severity: Severity,
    code: Option<&'static str>,
    title: String,
    message: Option<String>,
    span: Option<Span>,
    help: Option<String>,
    notes: Vec<String>,
}

impl DiagnosticBuilder {
    /// Create a new diagnostic builder.
    pub fn new(severity: Severity, title: impl Into<String>) -> Self {
        Self {
            severity,
            code: None,
            title: title.into(),
            message: None,
            span: None,
            help: None,
            notes: Vec::new(),
        }
    }

    /// Create a new error diagnostic builder.
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(Severity::Error, title)
    }

    /// Create a new warning diagnostic builder.
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(Severity::Warning, title)
    }

    /// Create a new note diagnostic builder.
    pub fn note(title: impl Into<String>) -> Self {
        Self::new(Severity::Note, title)
    }

    /// Set the error code.
    pub fn code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// Set the detailed message.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set the source span.
    pub fn span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Set the help text.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Add a note.
    pub fn add_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Build the diagnostic.
    pub fn build(self) -> Diagnostic {
        Diagnostic {
            severity: self.severity,
            code: self.code,
            title: self.title.clone(),
            message: self.message.unwrap_or_else(|| self.title.clone()),
            span: self.span,
            help: self.help,
            notes: self.notes,
        }
    }
}

/// Trait for types that can be converted to diagnostics.
///
/// Implement this trait to enable consistent error formatting across
/// all error types in the compiler.
pub trait Reportable {
    /// Convert to a Diagnostic for user display.
    fn to_diagnostic(&self) -> Diagnostic;

    /// Format the diagnostic for terminal output.
    fn report(&self, source: &str) -> String {
        self.to_diagnostic().format(source)
    }
}
