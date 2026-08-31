// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Anchoring a source text to the location it logically belongs to.
//!
//! Text that is checked or patched need not exist on disk, but imports like
//! `local.foo` still have to resolve from somewhere and diagnostics still have
//! to say which file they are about. Both answers come from an optional path,
//! and both are computed here so that every command anchors text the same way.

use std::path::{Path, PathBuf};

use crate::pipeline::Pipeline;

/// Build a pipeline that resolves imports and reports diagnostics as `path` says.
///
/// Every command that runs the frontend over text goes through here, so a check
/// of a text and a patch's own check of the same text cannot disagree about
/// where its imports come from.
///
/// Without a path the text belongs nowhere in particular: imports resolve from
/// the working directory and diagnostics carry no file.
pub fn pipeline_for(path: Option<&Path>) -> Pipeline {
    let Some(path) = path else {
        return Pipeline::new();
    };

    let absolute = absolute_path(path);
    let mut pipeline = Pipeline::new().with_source_path(absolute.display().to_string());
    if let Some(directory) = absolute.parent() {
        pipeline = pipeline.with_source_dir(directory.to_path_buf());
    }
    pipeline
}

/// Resolve `path` to an absolute location, whether or not it exists yet.
///
/// Canonicalizing answers this for a file already on disk. A path naming a file
/// that has not been written — a candidate edit, say — cannot be canonicalized,
/// and is resolved against the working directory instead, because taking the
/// parent of a bare relative name yields an empty directory that no import can
/// resolve against. A relative path is therefore anchored to the working
/// directory as it stands when the text is checked.
fn absolute_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(directory) => directory.join(path),
        Err(_) => path.to_path_buf(),
    }
}
