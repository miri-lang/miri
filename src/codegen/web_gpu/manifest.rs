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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<KernelSpec>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingSpec {
    pub name: String,
    pub access: String, // "read" or "read_write"
}

impl Manifest {
    /// Serialize the manifest to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
