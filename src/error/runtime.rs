// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared runtime error messages.
//!
//! These error messages are used by both the interpreter and compiled code
//! to ensure consistent error reporting.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable};
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
            Self::DivisionByZero => ErrorProperties::simple("E0400", "Division by Zero")
                .with_message("attempt to divide by zero"),
            Self::RemainderByZero => ErrorProperties::simple("E0401", "Remainder by Zero")
                .with_message("attempt to calculate the remainder with a divisor of zero"),
            Self::Overflow => ErrorProperties::simple("E0402", "Integer Overflow")
                .with_message("integer overflow"),
            Self::InvalidOperand { op, operand } => {
                ErrorProperties::simple("E0405", "Invalid Operand")
                    .with_message(format!("Invalid operand for {}: {}", op, operand))
            }
        }
    }
}

impl Reportable for RuntimeError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::from_props(self.properties(), None, None)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let props = self.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for RuntimeError {}
