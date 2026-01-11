// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Code generation errors.
//!
//! This module provides unified error types for all code generation backends
//! (Cranelift, LLVM, etc.). Errors are consolidated here for consistent
//! formatting and reporting.

use crate::error::codes;
use crate::error::diagnostic::{Diagnostic, Reportable, Severity};
use thiserror::Error;

/// Unified error type for all code generation backends.
#[derive(Debug, Error)]
pub enum CodegenError {
    /// Failed to create target ISA (instruction set architecture).
    #[error("Failed to create target ISA: {0}")]
    TargetIsa(String),

    /// Failed to create code generation module.
    #[error("Failed to create module: {0}")]
    Module(String),

    /// Failed to declare a function.
    #[error("Failed to declare function '{name}': {details}")]
    DeclareFunction { name: String, details: String },

    /// Failed to define a function.
    #[error("Failed to define function '{name}': {details}")]
    DefineFunction { name: String, details: String },

    /// Failed to translate MIR to backend IR.
    #[error("Failed to translate function '{name}': {details}")]
    Translation { name: String, details: String },

    /// Failed to emit object file.
    #[error("Failed to emit object file: {0}")]
    Emit(String),

    /// Backend is not yet supported.
    #[error("{backend} backend is not yet supported. Stay tuned!")]
    NotSupported { backend: String },

    /// Internal backend error.
    #[error("Internal codegen error: {0}")]
    Internal(String),
}

impl CodegenError {
    /// Get the error code for this codegen error.
    pub fn code(&self) -> &'static str {
        match self {
            CodegenError::TargetIsa(_) => codes::codegen::TARGET_ISA,
            CodegenError::Module(_) => codes::codegen::MODULE_CREATION,
            CodegenError::DeclareFunction { .. } => codes::codegen::FUNCTION_DECLARATION,
            CodegenError::DefineFunction { .. } => codes::codegen::FUNCTION_DEFINITION,
            CodegenError::Translation { .. } => codes::codegen::TRANSLATION,
            CodegenError::Emit(_) => codes::codegen::EMIT,
            CodegenError::NotSupported { .. } => codes::codegen::NOT_SUPPORTED,
            CodegenError::Internal(_) => codes::codegen::EMIT, // Reuse emit code for internal
        }
    }

    /// Get the human-readable title for this error.
    pub fn title(&self) -> &'static str {
        match self {
            CodegenError::TargetIsa(_) => "Target ISA Error",
            CodegenError::Module(_) => "Module Creation Error",
            CodegenError::DeclareFunction { .. } => "Function Declaration Error",
            CodegenError::DefineFunction { .. } => "Function Definition Error",
            CodegenError::Translation { .. } => "Translation Error",
            CodegenError::Emit(_) => "Emit Error",
            CodegenError::NotSupported { .. } => "Backend Not Supported",
            CodegenError::Internal(_) => "Internal Codegen Error",
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
        Diagnostic {
            severity: Severity::Error,
            code: Some(self.code()),
            title: self.title().to_string(),
            message: self.to_string(),
            span: None, // Codegen errors don't have source spans
            help: None,
            notes: Vec::new(),
        }
    }
}
