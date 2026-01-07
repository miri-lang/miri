// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! LLVM backend (stub).
//!
//! This module provides a placeholder LLVM backend that returns an error
//! indicating that LLVM support is not yet implemented.

use crate::codegen::backend::{Backend, CompiledArtifact};
use crate::mir::Body;
use std::fmt;
use thiserror::Error;

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

/// Errors from the LLVM backend.
#[derive(Debug, Error)]
pub enum LlvmError {
    /// LLVM backend is not yet implemented.
    #[error("LLVM backend is not yet supported. Stay tuned!")]
    NotYetSupported,
}

impl Backend for LlvmBackend {
    type Error = LlvmError;
    type Options = LlvmOptions;

    fn compile(
        &self,
        _bodies: &[(&str, &Body)],
        _options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        Err(LlvmError::NotYetSupported)
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
