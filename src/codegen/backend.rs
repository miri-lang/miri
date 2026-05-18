// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct MockBackend {
        artifact_bytes: Vec<u8>,
    }

    #[derive(Debug, Default)]
    struct MockOptions;

    impl Backend for MockBackend {
        type Error = std::io::Error;
        type Options = MockOptions;

        fn compile(
            &self,
            _bodies: &[(&str, &Body)],
            _options: &Self::Options,
        ) -> Result<CompiledArtifact, Self::Error> {
            Ok(CompiledArtifact::object_file(self.artifact_bytes.clone()))
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    #[test]
    fn compiled_artifact_object_file_sets_format() {
        let artifact = CompiledArtifact::object_file(vec![0xDE, 0xAD]);
        assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
        assert_eq!(artifact.bytes, vec![0xDE, 0xAD]);
    }

    #[test]
    fn compiled_artifact_new_preserves_format() {
        let artifact = CompiledArtifact::new(vec![1, 2, 3], ArtifactFormat::Executable);
        assert_eq!(artifact.format, ArtifactFormat::Executable);
        assert_eq!(artifact.bytes, vec![1, 2, 3]);
    }

    #[test]
    fn backend_trait_round_trips_through_mock_implementation() {
        let backend = MockBackend {
            artifact_bytes: vec![0x4D, 0x49],
        };
        let artifact = backend
            .compile(&[], &MockOptions)
            .expect("mock backend always succeeds");
        assert_eq!(backend.name(), "mock");
        assert_eq!(artifact.bytes, vec![0x4D, 0x49]);
        assert_eq!(artifact.format, ArtifactFormat::ObjectFile);
    }

    #[test]
    fn artifact_format_variants_are_distinct() {
        assert_ne!(ArtifactFormat::ObjectFile, ArtifactFormat::Executable);
        assert_ne!(ArtifactFormat::Executable, ArtifactFormat::SharedLibrary);
        assert_ne!(ArtifactFormat::ObjectFile, ArtifactFormat::SharedLibrary);
    }
}
