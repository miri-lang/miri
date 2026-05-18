// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Code generation errors.
//!
//! This module provides unified error types for all code generation backends
//! (Cranelift, LLVM, etc.). Errors are consolidated here for consistent
//! formatting and reporting.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable};
use std::fmt;

/// Unified error type for all code generation backends.
#[derive(Debug)]
pub enum CodegenError {
    /// Failed to create target ISA (instruction set architecture).
    TargetIsa(String),

    /// Failed to create code generation module.
    Module(String),

    /// Failed to declare a function.
    DeclareFunction { name: String, details: String },

    /// Failed to define a function.
    DefineFunction { name: String, details: String },

    /// Failed to translate MIR to backend IR.
    Translation { name: String, details: String },

    /// Failed to emit object file.
    Emit(String),

    /// Backend is not yet supported.
    NotSupported { backend: String },

    /// Internal backend error.
    Internal(String),
}

impl CodegenError {
    pub fn properties(&self) -> ErrorProperties {
        match self {
            CodegenError::TargetIsa(msg) => ErrorProperties::simple("E0300", "Target ISA Error")
                .with_message(format!("Failed to create target ISA: {}", msg)),
            CodegenError::Module(msg) => ErrorProperties::simple("E0301", "Module Creation Error")
                .with_message(format!("Failed to create module: {}", msg)),
            CodegenError::DeclareFunction { name, details } => {
                ErrorProperties::simple("E0302", "Function Declaration Error").with_message(
                    format!("Failed to declare function '{}': {}", name, details),
                )
            }
            CodegenError::DefineFunction { name, details } => {
                ErrorProperties::simple("E0303", "Function Definition Error")
                    .with_message(format!("Failed to define function '{}': {}", name, details))
            }
            CodegenError::Translation { name, details } => {
                ErrorProperties::simple("E0304", "Translation Error").with_message(format!(
                    "Failed to translate function '{}': {}",
                    name, details
                ))
            }
            CodegenError::Emit(msg) => ErrorProperties::simple("E0305", "Emit Error")
                .with_message(format!("Failed to emit object file: {}", msg)),
            CodegenError::NotSupported { backend } => {
                ErrorProperties::simple("E0306", "Backend Not Supported").with_message(format!(
                    "{} backend is not yet available. Use the Cranelift backend (default) instead.",
                    backend
                ))
            }
            CodegenError::Internal(msg) => {
                ErrorProperties::simple("E0307", "Internal Codegen Error")
                    .with_message(format!("Internal codegen error: {}", msg))
            }
        }
    }

    // Constructors for backward compatibility with CraneliftError

    /// Create a TargetIsa error.
    pub fn target_isa(msg: impl Into<String>) -> Self {
        CodegenError::TargetIsa(msg.into())
    }

    /// Create a Module error.
    pub fn module(msg: impl Into<String>) -> Self {
        CodegenError::Module(msg.into())
    }

    /// Create a DeclareFunction error.
    pub fn declare_function(name: impl Into<String>, details: impl Into<String>) -> Self {
        CodegenError::DeclareFunction {
            name: name.into(),
            details: details.into(),
        }
    }

    /// Create a DefineFunction error.
    pub fn define_function(name: impl Into<String>, details: impl Into<String>) -> Self {
        CodegenError::DefineFunction {
            name: name.into(),
            details: details.into(),
        }
    }

    /// Create a Translation error.
    pub fn translation(name: impl Into<String>, details: impl Into<String>) -> Self {
        CodegenError::Translation {
            name: name.into(),
            details: details.into(),
        }
    }

    /// Create an Emit error.
    pub fn emit(msg: impl Into<String>) -> Self {
        CodegenError::Emit(msg.into())
    }

    /// Create a NotSupported error for a specific backend.
    pub fn not_supported(backend: impl Into<String>) -> Self {
        CodegenError::NotSupported {
            backend: backend.into(),
        }
    }
}

impl Reportable for CodegenError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::from_props(self.properties(), None, None)
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let props = self.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for CodegenError {}
