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

/// One source-map span: the Miri source that produced a given WGSL line.
///
/// `wgsl_line` is 1-based into the emitted module text; `miri_offset` is a byte
/// offset into the original Miri source. Consumers (the website, a debugger)
/// convert the offset to a line/column against the source they display. Entries
/// are sorted by `wgsl_line`; each applies until the next, so a WGSL line with
/// no exact entry inherits the nearest preceding one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WgslSourceSpan {
    /// 1-based line in the emitted WGSL module.
    pub wgsl_line: u32,
    /// Byte offset into the Miri source that produced the line.
    pub miri_offset: usize,
}

/// A compiled WGSL module and its source map.
#[derive(Debug, Clone)]
pub struct WgslModule {
    /// The WGSL module text.
    pub wgsl: String,
    /// WGSL-line → Miri-offset spans (see [`WgslSourceSpan`]).
    pub source_map: Vec<WgslSourceSpan>,
}

/// Emit a WGSL module for `bodies` and return its text together with a source
/// map back to the Miri source. Used by the web-gpu bundle emitter so the
/// website can highlight the Miri line that produced a given WGSL line.
pub fn compile_module(
    bodies: &[(&str, &Body)],
    options: &WgslOptions,
) -> Result<WgslModule, CodegenError> {
    let (wgsl, source_map) = emit_module(bodies, options)?.finish_with_map();
    Ok(WgslModule { wgsl, source_map })
}

/// Shared module-emission core: run every body through the emitter and return
/// it, ready for `finish` (WGSL only) or `finish_with_map` (WGSL + source map).
fn emit_module(
    bodies: &[(&str, &Body)],
    options: &WgslOptions,
) -> Result<emitter::Emitter, CodegenError> {
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
                emitter.emit_helper(name, body)?;
            }
            ExecutionModel::Cpu | ExecutionModel::Async => {}
        }
    }

    Ok(emitter)
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
        let wgsl = emit_module(bodies, options)?.finish();
        Ok(CompiledArtifact::new(
            wgsl.into_bytes(),
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
