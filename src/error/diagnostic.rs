// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Core diagnostic types for unified error and warning reporting.
//!
//! This module provides the foundational types for the error infrastructure:
//! - [`Severity`] - Error, Warning, or Note level (re-exported from diagnostics)
//! - [`Diagnostic`] - Rich diagnostic message with all context
//! - [`Reportable`] - Trait for types that can produce diagnostics

pub use crate::diagnostics::Severity;

use crate::diagnostics::json::{JsonDiagnostic, JsonRelated};
use crate::diagnostics::repair::RepairRequest;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;

/// The official URL for reporting internal compiler errors.
pub const BUG_REPORT_URL: &str = "https://github.com/miri-lang/miri/issues";

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
    /// Error code for documentation/tooling (e.g., "MER_NAM_001", "MER_TYP_024").
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
    /// Optional (file_path, source_text) for errors originating from imported files.
    /// When present, the formatter uses this source instead of the main file's source.
    pub source_override: Option<(String, String)>,
    /// Expected type/value for type mismatch errors.
    pub expected: Option<String>,
    /// Actual type/value for type mismatch errors.
    pub actual: Option<String>,
    /// A repair recorded where this diagnostic was raised, when the correct
    /// edit is determined.
    pub repair: Option<RepairRequest>,
}

/// Consolidated error properties to keep widely scattered match statements in check.
#[derive(Debug, Clone)]
pub struct ErrorProperties {
    pub code: DiagnosticCode,
    pub title: &'static str,
    pub message: Option<String>,
    pub help: Option<String>,
}

impl ErrorProperties {
    /// Build from a registry code; message and help left empty.
    ///
    /// The title is read from the registry rather than passed in, so an error's
    /// code and the text shown beside it cannot drift apart.
    pub fn simple(code: DiagnosticCode) -> Self {
        Self {
            code,
            title: code.title(),
            message: None,
            help: None,
        }
    }

    /// Set the detailed message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set the help text.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Set the help text when there is one, leaving it unset otherwise.
    ///
    /// Coded diagnostics carry help as `Option<String>` because it comes from
    /// the call site rather than the registry; this keeps that from expanding
    /// into a `match` at every such arm.
    pub fn with_optional_help(mut self, help: Option<String>) -> Self {
        if let Some(help) = help {
            self.help = Some(help);
        }
        self
    }
}

/// Builds the properties for a coded diagnostic: the registry supplies the
/// title, the call site supplies the message and any help. Shared by every
/// error kind that carries a `Coded` variant.
pub fn coded_properties(
    code: DiagnosticCode,
    message: &str,
    help: &Option<String>,
) -> ErrorProperties {
    ErrorProperties::simple(code)
        .with_message(message.to_string())
        .with_optional_help(help.clone())
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

    /// Build an error diagnostic from `ErrorProperties`, attaching the given span
    /// and optional source override.
    pub fn from_props(
        props: ErrorProperties,
        span: Option<Span>,
        source_override: Option<(String, String)>,
    ) -> Self {
        let title = props.title.to_string();
        let message = props.message.unwrap_or_else(|| title.clone());
        Self {
            severity: Severity::Error,
            code: Some(props.code.as_str()),
            title,
            message,
            span,
            help: props.help,
            notes: Vec::new(),
            source_override,
            expected: None,
            actual: None,
            repair: None,
        }
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
    source_override: Option<(String, String)>,
    expected: Option<String>,
    actual: Option<String>,
    repair: Option<RepairRequest>,
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
            source_override: None,
            expected: None,
            actual: None,
            repair: None,
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

    /// Set source override for errors from imported files.
    pub fn source_override(mut self, file_path: String, source_text: String) -> Self {
        self.source_override = Some((file_path, source_text));
        self
    }

    /// Set the expected value for type mismatch errors.
    pub fn expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// Set the actual value for type mismatch errors.
    pub fn actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    /// Attach a repair recorded by the check raising this diagnostic.
    pub fn repair(mut self, repair: RepairRequest) -> Self {
        self.repair = Some(repair);
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
            source_override: self.source_override,
            expected: self.expected,
            actual: self.actual,
            repair: self.repair,
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

/// Convert a Diagnostic to its JSON representation.
///
/// The `source` parameter is the main source file's content; when a diagnostic
/// has a `source_override`, line/column are computed against that override
/// instead. The `source_path` is used as the file path for diagnostics that
/// do not carry an override.
pub fn to_json(diag: &Diagnostic, source: &str, source_path: Option<&str>) -> JsonDiagnostic {
    use crate::error::format::effective_source_and_label;

    let (effective_source, file_label) = effective_source_and_label(diag, source, source_path);

    let (line, column, length) = if let Some(span) = diag.span {
        let (line_num, col_num, _) =
            crate::error::syntax::find_line_info(effective_source, span.start);
        let len = span.end.saturating_sub(span.start);
        (Some(line_num), Some(col_num), Some(len))
    } else {
        (None, None, None)
    };

    let related = diag
        .notes
        .iter()
        .map(|note| JsonRelated {
            severity: "note".to_string(),
            message: note.clone(),
            code: None,
            path: None,
            line: None,
            column: None,
        })
        .collect();

    JsonDiagnostic {
        severity: diag.severity.as_str().to_string(),
        code: diag.code.map(|c| c.to_string()),
        message: diag.message.clone(),
        path: file_label.map(|p| p.to_string()),
        line,
        column,
        length,
        expected: diag.expected.clone(),
        actual: diag.actual.clone(),
        help: diag.help.clone(),
        fix_safety: None,
        repair: diag
            .repair
            .as_ref()
            .and_then(|request| request.project(file_label.unwrap_or_default(), effective_source)),
        related,
    }
}
