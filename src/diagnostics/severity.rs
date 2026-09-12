// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic severity levels.
//!
//! This module defines the severity hierarchy used across all compiler diagnostics.

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Hard error - compilation stops.
    Error,
    /// Warning - compilation continues, user should address.
    Warning,
    /// Note - reports on the command rather than on the source, either as
    /// additional context for another diagnostic or on its own. Compilation
    /// continues and there is nothing in the file for the author to address.
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
