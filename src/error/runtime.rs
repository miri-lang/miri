// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Shared runtime error messages.
//!
//! These error messages are used by both the interpreter and compiled code
//! to ensure consistent error reporting.

use crate::error::codes;
use crate::error::diagnostic::{Diagnostic, Reportable, Severity};
use std::fmt;

/// Runtime errors that can occur during program execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// Attempted to divide by zero.
    DivisionByZero,
    /// Attempted to calculate remainder with zero divisor.
    RemainderByZero,
}

impl RuntimeError {
    /// Get the human-readable error message.
    pub fn message(&self) -> &'static str {
        match self {
            Self::DivisionByZero => "attempt to divide by zero",
            Self::RemainderByZero => "attempt to calculate the remainder with a divisor of zero",
        }
    }

    /// Get the error code for this runtime error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DivisionByZero => codes::runtime::DIVISION_BY_ZERO,
            Self::RemainderByZero => codes::runtime::REMAINDER_BY_ZERO,
        }
    }

    /// Get the human-readable title for this error.
    pub fn title(&self) -> &'static str {
        match self {
            Self::DivisionByZero => "Division by Zero",
            Self::RemainderByZero => "Remainder by Zero",
        }
    }
}

impl Reportable for RuntimeError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: Some(self.code()),
            title: self.title().to_string(),
            message: self.message().to_string(),
            span: None, // Runtime errors don't have source spans
            help: None,
            notes: Vec::new(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for RuntimeError {}
