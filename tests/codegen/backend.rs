// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
use miri::mir::Body;

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
