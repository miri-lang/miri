// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Shared runtime error messages.
//!
//! These error messages are used by both the interpreter and compiled code
//! to ensure consistent error reporting.

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
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for RuntimeError {}
