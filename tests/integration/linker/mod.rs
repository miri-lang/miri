// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::compiler::CompilerError;
use miri::pipeline::{BuildOptions, Pipeline};
use std::path::PathBuf;
use std::sync::Mutex;

pub mod resolution;

/// Mutex to serialize tests that mutate process-wide environment variables.
pub(super) static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Assert that building `source` with the given linker path results in a
/// `Codegen` error whose message contains both `"Failed to run linker"` and
/// `linker_path`.
pub(super) fn assert_linker_error(source: &str, linker_path: &str) {
    let pipeline = Pipeline::new();
    let opts = BuildOptions {
        out_path: Some(PathBuf::from("/tmp/miri_linker_test_output")),
        ..Default::default()
    };

    match pipeline.build(source, &opts) {
        Err(CompilerError::Codegen(msg)) => {
            assert!(
                msg.contains("Failed to run linker"),
                "Expected 'Failed to run linker' in error message, got: {}",
                msg
            );
            assert!(
                msg.contains(linker_path),
                "Expected linker path '{}' in error message, got: {}",
                linker_path,
                msg
            );
        }
        other => panic!(
            "Expected Codegen error for bogus linker '{}', got: {:?}",
            linker_path, other
        ),
    }
}
