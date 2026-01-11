// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, Reportable, Severity};
use crate::error::format::format_diagnostic;
use crate::error::syntax::Span;

#[derive(Debug, PartialEq, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

impl TypeError {
    pub fn new(message: String, span: Span) -> Self {
        Self {
            message,
            span,
            help: None,
        }
    }

    pub fn with_help(mut self, help: String) -> Self {
        self.help = Some(help);
        self
    }

    /// Report the error using the legacy format function.
    pub fn report(&self, source: &str) -> String {
        Reportable::report(self, source)
    }
}

impl Reportable for TypeError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: None, // Type errors use dynamic messages
            title: self.message.clone(),
            message: self.message.clone(),
            span: Some(self.span.clone()),
            help: self.help.clone(),
            notes: Vec::new(),
        }
    }

    fn report(&self, source: &str) -> String {
        format_diagnostic(
            source,
            &self.span,
            &self.message,
            "error",
            self.help.as_deref(),
        )
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
