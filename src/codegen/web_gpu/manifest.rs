// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! JSON manifest emitter for web-gpu bundles.
//!
//! Produces a JSON manifest consumable by the runtime driver in `assets/web/miri-gpu.js`.
//! The manifest describes all buffers, kernels, and animation parameters.

use serde::Serialize;

/// Manifest schema matching the runtime's expectations.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub name: String,
    pub canvas: CanvasSpec,
    pub buffers: Vec<BufferSpec>,
    pub seed: Vec<KernelSpec>,
    #[serde(rename = "framePasses", skip_serializing_if = "Vec::is_empty")]
    pub frame_passes: Vec<KernelSpec>,
    pub paint: String,
    #[serde(rename = "paintMode", skip_serializing_if = "Option::is_none")]
    pub paint_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BufferSpec {
    pub name: String,
    #[serde(rename = "elemType")]
    pub elem_type: String,
    pub length: u32,
    #[serde(rename = "initialData")]
    pub initial_data: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputFieldSpec {
    pub name: String,
    pub ty: String,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelSpec {
    #[serde(rename = "entryPoint")]
    pub entry_point: String,
    pub wgsl: String,
    pub workgroups: [u32; 3],
    pub bindings: Vec<BindingSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<InputFieldSpec>>,
    /// WGSL-line → Miri-line map for source highlighting. Omitted when empty
    /// (e.g. the original source was unavailable at bundle time).
    #[serde(rename = "sourceMap", skip_serializing_if = "Vec::is_empty")]
    pub source_map: Vec<SourceMapEntry>,
}

/// One entry of a kernel's WGSL → Miri source map. Both lines are 1-based:
/// `wgsl` into the kernel's `wgsl` text, `miri` into the displayed Miri source.
#[derive(Debug, Clone, Serialize)]
pub struct SourceMapEntry {
    pub wgsl: u32,
    pub miri: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingSpec {
    pub name: String,
    pub access: String, // "read" or "read_write"
    /// Whether this pass writes the buffer. `access` is only the WGSL storage
    /// qualifier and is forced to `read_write` for atomic buffers, so consumers
    /// that need the pass's real data flow — state-pair inference in the runtime
    /// driver — read this instead.
    pub writes: bool,
}

impl Manifest {
    /// Serialize the manifest to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
