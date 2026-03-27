// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Code generation errors.
//!
//! This module provides unified error types for all code generation backends
//! (Cranelift, LLVM, etc.). Errors are consolidated here for consistent
//! formatting and reporting.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
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
            CodegenError::TargetIsa(msg) => ErrorProperties {
                code: "E0300",
                title: "Target ISA Error",
                message: Some(format!("Failed to create target ISA: {}", msg)),
                help: None,
            },
            CodegenError::Module(msg) => ErrorProperties {
                code: "E0301",
                title: "Module Creation Error",
                message: Some(format!("Failed to create module: {}", msg)),
                help: None,
            },
            CodegenError::DeclareFunction { name, details } => ErrorProperties {
                code: "E0302",
                title: "Function Declaration Error",
                message: Some(format!(
                    "Failed to declare function '{}': {}",
                    name, details
                )),
                help: None,
            },
            CodegenError::DefineFunction { name, details } => ErrorProperties {
                code: "E0303",
                title: "Function Definition Error",
                message: Some(format!("Failed to define function '{}': {}", name, details)),
                help: None,
            },
            CodegenError::Translation { name, details } => ErrorProperties {
                code: "E0304",
                title: "Translation Error",
                message: Some(format!(
                    "Failed to translate function '{}': {}",
                    name, details
                )),
                help: None,
            },
            CodegenError::Emit(msg) => ErrorProperties {
                code: "E0305",
                title: "Emit Error",
                message: Some(format!("Failed to emit object file: {}", msg)),
                help: None,
            },
            CodegenError::NotSupported { backend } => ErrorProperties {
                code: "E0306",
                title: "Backend Not Supported",
                message: Some(format!(
                    "{} backend is not yet available. Use the Cranelift backend (default) instead.",
                    backend
                )),
                help: None,
            },
            CodegenError::Internal(msg) => ErrorProperties {
                code: "E0307",
                title: "Internal Codegen Error",
                message: Some(format!("Internal codegen error: {}", msg)),
                help: None,
            },
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
        let props = self.properties();
        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span: None, // Codegen errors don't have source spans
            help: props.help,
            notes: Vec::new(),
            source_override: None,
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let props = self.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for CodegenError {}
