// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri build --target web-gpu` bundle emitter.
//!
//! Takes the MIR lowered by `Pipeline::build` and produces a self-contained
//! directory consumable by any static file server:
//!
//! ```text
//! <out>/
//!   index.html              — harness with <canvas> and dispatch boot code
//!   miri_gpu_runtime.js     — JS runtime shim (copied from assets/web/)
//!   kernels/<entry>.wgsl    — one shader per GPU kernel body
//! ```
//!
//! The HTML entry runs each kernel through the runtime and renders the first
//! compute output as pixels on the canvas. Native host code is intentionally
//! NOT compiled — the host story is JS-on-the-page until M9 Task 6 (full
//! WASM host compilation).

mod html;

use crate::codegen::wgsl::{WgslBackend, WgslOptions};
use crate::codegen::Backend;
use crate::error::compiler::CompilerError;
use crate::mir::backend::BackendMetadata;
use crate::mir::{Body, ExecutionModel};
use std::fs;
use std::path::{Path, PathBuf};

const RUNTIME_JS: &str = include_str!("../../../assets/web/miri_gpu_runtime.js");
const RUNTIME_JS_FILENAME: &str = "miri_gpu_runtime.js";
const INDEX_HTML_FILENAME: &str = "index.html";
const KERNELS_DIRNAME: &str = "kernels";

/// One compiled GPU entry point and its on-disk artifact.
#[derive(Debug)]
struct KernelArtifact {
    entry_point: String,
    file_name: String,
    workgroup_size: [u32; 3],
}

/// Emit the web-gpu bundle to disk. Returns the path of the produced
/// `index.html`. The caller chooses `out_path`: it is treated as a directory
/// to fill; `None` falls back to a unique tempdir.
pub fn emit_bundle(
    mir_bodies: &[(String, Body)],
    out_path: Option<&PathBuf>,
) -> Result<PathBuf, CompilerError> {
    let kernels = extract_kernels(mir_bodies);
    if kernels.is_empty() {
        return Err(CompilerError::Codegen(
            "--target web-gpu requires the program to declare at least one GPU kernel \
             (no GPU kernels were found in the source)"
                .to_string(),
        ));
    }

    let bundle_dir = resolve_bundle_dir(out_path)?;
    fs::create_dir_all(&bundle_dir)?;

    let kernels_dir = bundle_dir.join(KERNELS_DIRNAME);
    fs::create_dir_all(&kernels_dir)?;

    let artifacts = compile_kernels(&kernels, &kernels_dir)?;

    fs::write(bundle_dir.join(RUNTIME_JS_FILENAME), RUNTIME_JS)?;

    let index_path = bundle_dir.join(INDEX_HTML_FILENAME);
    let html_text = html::render(&artifacts);
    fs::write(&index_path, html_text)?;

    Ok(index_path)
}

fn resolve_bundle_dir(out_path: Option<&PathBuf>) -> Result<PathBuf, CompilerError> {
    match out_path {
        Some(path) => Ok(path.clone()),
        None => {
            let temp = tempfile::Builder::new()
                .prefix("miri_web_gpu_")
                .tempdir()
                .map_err(|err| {
                    CompilerError::Codegen(format!("Failed to create bundle directory: {}", err))
                })?;
            #[allow(deprecated)]
            Ok(temp.into_path())
        }
    }
}

fn extract_kernels(mir_bodies: &[(String, Body)]) -> Vec<(&str, &Body)> {
    mir_bodies
        .iter()
        .filter(|(_, body)| matches!(body.execution_model, ExecutionModel::GpuKernel))
        .map(|(name, body)| (name.as_str(), body))
        .collect()
}

fn compile_kernels(
    kernels: &[(&str, &Body)],
    kernels_dir: &Path,
) -> Result<Vec<KernelArtifact>, CompilerError> {
    let backend = WgslBackend;
    let options = WgslOptions::default();
    let mut artifacts = Vec::with_capacity(kernels.len());

    for (name, body) in kernels {
        let artifact = backend
            .compile(&[(*name, *body)], &options)
            .map_err(|err| CompilerError::Codegen(err.to_string()))?;
        let wgsl_text = String::from_utf8(artifact.bytes).map_err(|err| {
            CompilerError::Codegen(format!(
                "WGSL backend produced non-UTF-8 output for kernel '{}': {}",
                name, err
            ))
        })?;

        let file_name = format!("{}.wgsl", name);
        fs::write(kernels_dir.join(&file_name), &wgsl_text)?;

        artifacts.push(KernelArtifact {
            entry_point: (*name).to_string(),
            file_name,
            workgroup_size: resolve_workgroup_size(body),
        });
    }

    Ok(artifacts)
}

fn resolve_workgroup_size(body: &Body) -> [u32; 3] {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.workgroup_size.unwrap_or([64, 1, 1]),
        None => [64, 1, 1],
    }
}
