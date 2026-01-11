// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Shared runtime error messages.
//!
//! These error messages are used by both the interpreter and compiled code
//! to ensure consistent error reporting.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
use std::fmt;

/// Runtime errors that can occur during program execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// Attempted to divide by zero.
    DivisionByZero,
    /// Attempted to calculate remainder with zero divisor.
    RemainderByZero,
    /// Integer overflow occurred.
    Overflow,
    /// Invalid operand for an operation.
    InvalidOperand { op: String, operand: String },
}

impl RuntimeError {
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::DivisionByZero => ErrorProperties {
                code: "E0400",
                title: "Division by Zero",
                message: Some("attempt to divide by zero".to_string()),
                help: None,
            },
            Self::RemainderByZero => ErrorProperties {
                code: "E0401",
                title: "Remainder by Zero",
                message: Some(
                    "attempt to calculate the remainder with a divisor of zero".to_string(),
                ),
                help: None,
            },
            Self::Overflow => ErrorProperties {
                code: "E0402",
                title: "Integer Overflow",
                message: Some("integer overflow".to_string()),
                help: None,
            },
            Self::InvalidOperand { op, operand } => ErrorProperties {
                code: "E0405",
                title: "Invalid Operand",
                message: Some(format!("Invalid operand for {}: {}", op, operand)),
                help: None,
            },
        }
    }
}

impl Reportable for RuntimeError {
    fn to_diagnostic(&self) -> Diagnostic {
        let props = self.properties();
        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span: None, // Runtime errors don't have source spans
            help: props.help,
            notes: Vec::new(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let props = self.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for RuntimeError {}
