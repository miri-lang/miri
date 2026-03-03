// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, Reportable, Severity, BUG_REPORT_URL};
use crate::error::format::format_diagnostic_full;
use crate::error::lowering::LoweringError;
use crate::error::syntax::SyntaxError;
use crate::error::type_error::TypeError;
use thiserror::Error;

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
    Type(TypeError),

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
}

impl CompilerError {
    /// Formats this error for terminal display using the given source code.
    ///
    /// All variants are routed through [`format_diagnostic_full`] to ensure
    /// consistent formatting and TTY-aware color output.
    pub fn report(&self, source: &str) -> String {
        match self {
            CompilerError::Lexer(e) | CompilerError::Parser(e) => {
                format_diagnostic_full(source, &e.to_diagnostic())
            }
            CompilerError::Type(e) => format_diagnostic_full(source, &e.to_diagnostic()),
            CompilerError::TypeErrors { errors, warnings } => {
                let mut parts: Vec<String> = warnings
                    .iter()
                    .map(|w| format_diagnostic_full(source, w))
                    .collect();
                parts.extend(
                    errors
                        .iter()
                        .map(|e| format_diagnostic_full(source, &e.to_diagnostic())),
                );
                parts.join("\n")
            }
            CompilerError::Lowering(e) => format_diagnostic_full(source, &e.to_diagnostic()),
            CompilerError::Io(e) => {
                let diagnostic = Diagnostic {
                    severity: Severity::Error,
                    code: None,
                    title: "I/O Error".to_string(),
                    message: format!("{}", e),
                    span: None,
                    help: None,
                    notes: Vec::new(),
                };
                format_diagnostic_full(source, &diagnostic)
            }
            CompilerError::FileNotFound(path) => {
                let diagnostic = Diagnostic {
                    severity: Severity::Error,
                    code: None,
                    title: "File Not Found".to_string(),
                    message: format!("File not found: {}", path),
                    span: None,
                    help: None,
                    notes: Vec::new(),
                };
                format_diagnostic_full(source, &diagnostic)
            }
            CompilerError::Internal(msg) => {
                let diagnostic = Diagnostic {
                    severity: Severity::Error,
                    code: None,
                    title: "Internal Compiler Error".to_string(),
                    message: msg.clone(),
                    span: None,
                    help: Some(format!("Please report this at {}", BUG_REPORT_URL)),
                    notes: Vec::new(),
                };
                format_diagnostic_full(source, &diagnostic)
            }
            CompilerError::Codegen(msg) => {
                let diagnostic = Diagnostic {
                    severity: Severity::Error,
                    code: None,
                    title: "Code Generation Error".to_string(),
                    message: msg.clone(),
                    span: None,
                    help: None,
                    notes: Vec::new(),
                };
                format_diagnostic_full(source, &diagnostic)
            }
            CompilerError::Runtime(msg) => {
                let diagnostic = Diagnostic {
                    severity: Severity::Error,
                    code: None,
                    title: "Runtime Error".to_string(),
                    message: msg.clone(),
                    span: None,
                    help: None,
                    notes: Vec::new(),
                };
                format_diagnostic_full(source, &diagnostic)
            }
        }
    }
}
