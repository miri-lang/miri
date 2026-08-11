// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub use crate::integration::utils;
use tempfile::TempDir;

pub mod basic;
pub mod errors;
pub mod rc;

/// Creates a temporary test file and returns the temp directory and the file path.
/// The caller is responsible for keeping the TempDir alive; when dropped, the temp
/// directory and all its contents are removed.
pub fn temp_test_file(name: &str) -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let path = temp_dir.path().join(name).to_string_lossy().to_string();
    (temp_dir, path)
}
