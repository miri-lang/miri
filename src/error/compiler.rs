// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, Reportable, Severity, BUG_REPORT_URL};
use crate::error::format::{format_diagnostic, format_diagnostic_with_color, ColorChoice};
use crate::error::lowering::LoweringError;
use crate::error::syntax::SyntaxError;
use crate::error::type_error::TypeError;
use thiserror::Error;

fn simple_diag(title: &str, message: String, help: Option<String>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: None,
        title: title.to_string(),
        message,
        span: None,
        help,
        notes: Vec::new(),
        source_override: None,
        expected: None,
        actual: None,
    }
}

/// Top-level error type encompassing all compiler pipeline errors.
#[derive(Error, Debug)]
pub enum CompilerError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Lexer Error: {0}")]
    Lexer(SyntaxError),

    #[error("Parser Error: {0}")]
    Parser(SyntaxError),

    #[error("Type Error: {0}")]
    Type(Box<TypeError>),

    #[error("Type Errors: {errors:?}")]
    TypeErrors {
        errors: Vec<TypeError>,
        warnings: Vec<Diagnostic>,
    },

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Internal compiler error: {0}")]
    Internal(String),

    #[error("Codegen Error: {0}")]
    Codegen(String),

    #[error("Lowering Error: {0}")]
    Lowering(LoweringError),

    #[error("Runtime Error: {0}")]
    Runtime(String),

    #[error("MIR Verification Error: {0}")]
    MirVerification(String),
}

impl CompilerError {
    /// Converts this error into a Vec of Diagnostics.
    ///
    /// This is the conversion funnel point: every CompilerError variant maps to
    /// one or more Diagnostic values. An EXHAUSTIVE match is required (no `_ =>`).
    pub fn to_diagnostics(&self) -> Vec<Diagnostic> {
        match self {
            CompilerError::Lexer(e) | CompilerError::Parser(e) => {
                vec![e.to_diagnostic()]
            }
            CompilerError::Type(e) => vec![e.to_diagnostic()],
            CompilerError::TypeErrors { errors, warnings } => {
                let mut diags: Vec<Diagnostic> = warnings.clone();
                diags.extend(errors.iter().map(|e| e.to_diagnostic()));
                diags
            }
            CompilerError::Lowering(e) => vec![e.to_diagnostic()],
            CompilerError::Io(e) => {
                vec![simple_diag("I/O Error", format!("{}", e), None)]
            }
            CompilerError::FileNotFound(path) => vec![simple_diag(
                "File Not Found",
                format!("File not found: {}", path),
                None,
            )],
            CompilerError::Internal(msg) => vec![simple_diag(
                "Internal Compiler Error",
                msg.clone(),
                Some(format!("Please report this at {}", BUG_REPORT_URL)),
            )],
            CompilerError::Codegen(msg) => {
                vec![simple_diag("Code Generation Error", msg.clone(), None)]
            }
            CompilerError::Runtime(msg) => {
                vec![simple_diag("Runtime Error", msg.clone(), None)]
            }
            CompilerError::MirVerification(msg) => vec![simple_diag(
                "MIR Verification Error",
                msg.clone(),
                Some(
                    "This indicates a bug in MIR lowering or Perceus RC insertion. \
                     Please report it."
                        .to_string(),
                ),
            )],
        }
    }

    /// Formats this error for terminal display using the given source code.
    ///
    /// All variants are routed through [`format_diagnostic_full`] to ensure
    /// consistent formatting and TTY-aware color output.
    pub fn report(&self, source: &str) -> String {
        self.report_with_path(source, None)
    }

    /// Like [`report`](Self::report), but includes the entry-point file path
    /// in error locations when no per-diagnostic `source_override` is set.
    pub fn report_with_path(&self, source: &str, source_path: Option<&str>) -> String {
        let fmt = |diag: &Diagnostic| format_diagnostic(source, diag, source_path);
        self.to_diagnostics()
            .iter()
            .map(&fmt)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Like [`report_with_path`](Self::report_with_path), but respects the given color choice.
    pub fn report_with_path_and_color(
        &self,
        source: &str,
        source_path: Option<&str>,
        color_choice: ColorChoice,
    ) -> String {
        let fmt = |diag: &Diagnostic| {
            format_diagnostic_with_color(source, diag, source_path, color_choice)
        };
        self.to_diagnostics()
            .iter()
            .map(&fmt)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
