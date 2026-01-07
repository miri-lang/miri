// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Backend abstraction for code generation.
//!
//! This module defines the `Backend` trait that all code generators implement,
//! enabling support for multiple backends (Cranelift, LLVM, GPU backends).

use crate::mir::Body;
use std::error::Error;
use std::fmt::Debug;

/// The format of a compiled artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactFormat {
    /// Object file (.o)
    ObjectFile,
    /// Executable binary
    Executable,
    /// Shared library (.so, .dylib, .dll)
    SharedLibrary,
}

/// A compiled artifact produced by a backend.
#[derive(Debug)]
pub struct CompiledArtifact {
    /// The raw bytes of the compiled artifact.
    pub bytes: Vec<u8>,
    /// The format of the artifact.
    pub format: ArtifactFormat,
}

impl CompiledArtifact {
    /// Create a new compiled artifact.
    pub fn new(bytes: Vec<u8>, format: ArtifactFormat) -> Self {
        Self { bytes, format }
    }

    /// Create an object file artifact.
    pub fn object_file(bytes: Vec<u8>) -> Self {
        Self::new(bytes, ArtifactFormat::ObjectFile)
    }
}

/// Code generation backend trait.
///
/// All code generation backends (Cranelift, LLVM, GPU) implement this trait.
/// The trait is designed to be extensible for different target architectures
/// and output formats.
///
/// # Type Parameters
///
/// - `Error`: The error type returned by compilation operations.
/// - `Options`: Backend-specific compilation options (optimization level, target, etc.).
pub trait Backend: Debug {
    /// The error type for this backend.
    type Error: Error + Send + Sync + 'static;

    /// Backend-specific compilation options.
    type Options: Default + Debug;

    /// Compile MIR bodies to a compiled artifact.
    ///
    /// # Arguments
    ///
    /// * `bodies` - A slice of (function_name, body) pairs to compile.
    /// * `options` - Backend-specific compilation options.
    ///
    /// # Returns
    ///
    /// A compiled artifact containing the generated code, or an error.
    fn compile(
        &self,
        bodies: &[(&str, &Body)],
        options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error>;

    /// Returns the name of this backend for display purposes.
    fn name(&self) -> &'static str;
}
