// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Error types for MIR lowering.

use crate::error::syntax::Span;
use crate::error::utils::format_diagnostic;

/// Errors that can occur during MIR lowering.
#[derive(Debug, Clone, PartialEq)]
pub struct LoweringError {
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

impl LoweringError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn unsupported_expression(desc: &str, span: Span) -> Self {
        Self::new(format!("unsupported expression: {}", desc), span)
    }

    pub fn unsupported_statement(desc: &str, span: Span) -> Self {
        Self::new(format!("unsupported statement: {}", desc), span)
    }

    pub fn undefined_variable(name: &str, span: Span) -> Self {
        Self::new(format!("undefined variable: {}", name), span)
    }

    pub fn type_not_found(expr_id: usize, span: Span) -> Self {
        Self::new(
            format!("type not found for expression ID {}", expr_id),
            span,
        )
    }

    pub fn break_outside_loop(span: Span) -> Self {
        Self::new("break statement outside of loop", span)
    }

    pub fn continue_outside_loop(span: Span) -> Self {
        Self::new("continue statement outside of loop", span)
    }

    pub fn unsupported_lhs(desc: &str, span: Span) -> Self {
        Self::new(format!("unsupported left-hand side: {}", desc), span)
    }

    pub fn report(&self, source: &str) -> String {
        format_diagnostic(
            source,
            &self.span,
            &self.message,
            "error",
            self.help.as_deref(),
        )
    }
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LoweringError {}
