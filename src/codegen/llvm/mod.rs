// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! LLVM backend (stub).
//!
//! This module provides a placeholder LLVM backend that returns an error
//! indicating that LLVM support is not yet implemented.

use crate::codegen::backend::{Backend, CompiledArtifact};
use crate::error::CodegenError;
use crate::mir::Body;
use std::fmt;

/// LLVM backend placeholder.
///
/// This backend is not yet implemented and will return an error when used.
#[derive(Debug, Default)]
pub struct LlvmBackend;

/// LLVM backend compilation options (placeholder).
#[derive(Debug, Default)]
pub struct LlvmOptions {
    /// Optimization level (0-3).
    pub opt_level: u8,
}

impl Backend for LlvmBackend {
    type Error = CodegenError;
    type Options = LlvmOptions;

    fn compile(
        &self,
        _bodies: &[(&str, &Body)],
        _options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        Err(CodegenError::not_supported("LLVM"))
    }

    fn name(&self) -> &'static str {
        "llvm"
    }
}

impl fmt::Display for LlvmBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LlvmBackend (not yet implemented)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::backend::Backend;

    #[test]
    fn name_reports_llvm() {
        assert_eq!(LlvmBackend.name(), "llvm");
    }

    #[test]
    fn compile_returns_not_supported_error() {
        let result = LlvmBackend.compile(&[], &LlvmOptions::default());
        let err = result.expect_err("LLVM backend stub must reject compile()");
        assert!(
            matches!(err, CodegenError::NotSupported { ref backend } if backend == "LLVM"),
            "expected CodegenError::NotSupported {{ backend: \"LLVM\" }}, got {err:?}",
        );
    }

    #[test]
    fn display_marks_unimplemented_status() {
        assert_eq!(
            format!("{}", LlvmBackend),
            "LlvmBackend (not yet implemented)"
        );
    }

    #[test]
    fn options_default_opt_level_is_zero() {
        assert_eq!(LlvmOptions::default().opt_level, 0);
    }
}
