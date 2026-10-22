// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Backend abstraction for code generation.
//!
//! This module defines the `Backend` trait that all code generators implement,
//! enabling support for multiple backends (Cranelift, LLVM, GPU backends).
//! It also defines the selection enums `CpuBackend` and `BuildTarget` for CLI integration.

use crate::error::SourceId;
use crate::mir::Body;
use std::error::Error;
use std::fmt::Debug;

/// CPU backend for code generation.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuBackend {
    /// Cranelift: Fast compilation, good for development (default)
    #[default]
    Cranelift,
    /// LLVM: Optimized compilation
    Llvm,
}

/// Selectable build target.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildTarget {
    /// Native executable for the host platform.
    #[default]
    Native,
    /// Browser-runnable WebGPU bundle (HTML + WGSL + JS runtime).
    #[value(name = "web-gpu")]
    WebGpu,
}

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
    /// The backend's own IR of every function it compiled, when its options
    /// asked for a listing.
    pub ir_listing: Option<String>,
    /// Where each compiled function's code came from, in the order the
    /// functions were compiled, when the backend's options asked for it.
    /// Empty otherwise.
    pub code_map: Vec<FunctionCodeMap>,
}

/// The source offsets one compiled function's machine code was generated
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCodeMap {
    /// The function's link symbol.
    pub symbol: String,
    /// The size of the function's machine code, in bytes.
    pub code_size: u32,
    /// The file the offsets of `ranges` index, as the body recorded it.
    pub source: Option<SourceId>,
    /// The function's code that came from a known place, ordered by offset
    /// and not overlapping.
    pub ranges: Vec<CodeRange>,
}

/// A run of machine code generated from one place in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRange {
    /// The first byte of the run, from the start of the function.
    pub start: u32,
    /// The byte after the last byte of the run, from the start of the function.
    pub end: u32,
    /// The byte offset into the source file the run was generated from.
    pub source_offset: u32,
}

impl CompiledArtifact {
    /// Create a new compiled artifact.
    pub fn new(bytes: Vec<u8>, format: ArtifactFormat) -> Self {
        Self {
            bytes,
            format,
            ir_listing: None,
            code_map: Vec::new(),
        }
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
