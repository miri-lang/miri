// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL code generation backend.
//!
//! Emits WebGPU Shading Language text for `gpu fn` bodies, mapping MIR
//! GPU intrinsics and storage classes onto the WGSL compute pipeline.
//! Intended to be consumed by a host runtime (Wasm/JS or native `wgpu`).

mod emitter;
mod types;

use crate::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
use crate::error::CodegenError;
use crate::mir::{Body, ExecutionModel};
use std::fmt;

/// Default workgroup size used when a kernel does not declare one.
const DEFAULT_WORKGROUP_SIZE: [u32; 3] = [64, 1, 1];

/// WGSL backend compilation options.
#[derive(Debug, Default)]
pub struct WgslOptions {
    /// Fallback workgroup size when the kernel lacks GPU metadata.
    pub default_workgroup_size: Option<[u32; 3]>,
}

/// WGSL backend.
///
/// Produces a `CompiledArtifact` whose `bytes` field is UTF-8 WGSL source.
/// The artifact format is reported as `ObjectFile` because the runtime
/// pipeline treats it as an opaque byte buffer to be embedded in HTML/JS.
#[derive(Debug, Default)]
pub struct WgslBackend;

impl Backend for WgslBackend {
    type Error = CodegenError;
    type Options = WgslOptions;

    fn compile(
        &self,
        bodies: &[(&str, &Body)],
        options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        let mut emitter = emitter::Emitter::new();
        let workgroup_default = options
            .default_workgroup_size
            .unwrap_or(DEFAULT_WORKGROUP_SIZE);

        for (name, body) in bodies {
            match body.execution_model {
                ExecutionModel::GpuKernel => {
                    emitter.emit_kernel(name, body, workgroup_default)?;
                }
                ExecutionModel::GpuDevice => {
                    return Err(CodegenError::Internal(format!(
                        "WGSL backend: emitting GpuDevice (non-entry) functions is not yet \
                         supported (function '{}')",
                        name
                    )));
                }
                ExecutionModel::Cpu | ExecutionModel::Async => {}
            }
        }

        Ok(CompiledArtifact::new(
            emitter.finish().into_bytes(),
            ArtifactFormat::ObjectFile,
        ))
    }

    fn name(&self) -> &'static str {
        "wgsl"
    }
}

impl fmt::Display for WgslBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WgslBackend")
    }
}
