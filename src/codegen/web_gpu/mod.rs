// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri build --target web-gpu` bundle emitter.
//!
//! Produces a self-contained directory with:
//! - A JSON manifest describing all buffers, kernels, and animation metadata
//! - miri-gpu.js runtime driver (reusable embeddable module)
//! - index.html harness for local development
//!
//! WGSL kernels are embedded in the manifest JSON under `seed[].wgsl` and
//! `frame.wgsl` (if present), not as separate files.

mod buffers;
mod manifest;

use crate::ast::types::{FrameFieldKind, FRAME_INPUT_FIELDS};
use crate::codegen::wgsl::types::is_atomic_element_buffer;
use crate::codegen::wgsl::{compile_module, WgslOptions};
use crate::error::compiler::CompilerError;
use crate::error::syntax::Span;
use crate::mir::backend::BackendMetadata;
use crate::mir::body::DeviceHandleId;
use crate::mir::{Body, ExecutionModel, LocalDecl};
use crate::type_checker::GpuBufferInit;
use buffers::{BufferTable, HostProgram};
use manifest::{
    BindingSpec, BufferSpec, CanvasSpec, InputFieldSpec, KernelSpec, Manifest, SourceMapEntry,
};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const MIRI_GPU_JS: &str = include_str!("../../../assets/web/miri-gpu.js");
const MIRI_GPU_JS_FILENAME: &str = "miri-gpu.js";
const MIRI_GPU_HEADLESS_JS: &str = include_str!("../../../assets/web/miri-gpu-headless.js");
const MIRI_GPU_HEADLESS_JS_FILENAME: &str = "miri-gpu-headless.js";
const INDEX_HTML_FILENAME: &str = "index.html";
/// Marks the bundle as an ES-module package so a JS runtime (Node/Deno) imports
/// the `.js` harness and headless runner as modules, not CommonJS.
const PACKAGE_JSON: &str = "{\n  \"type\": \"module\"\n}\n";
const PACKAGE_JSON_FILENAME: &str = "package.json";

/// Per-binding metadata for a kernel's storage buffer.
#[derive(Debug, Clone)]
struct BufferBinding {
    /// Manifest name of the device buffer bound here.
    name: String,
    element_type: &'static str,
    read_only: bool,
    /// Whether the kernel writes this buffer. `read_only` is the WGSL storage
    /// qualifier and is forced false for atomic buffers, so it cannot answer
    /// the data-flow question the runtime's state-pair inference asks.
    writes: bool,
}

/// A kernel ready to compile: its body, the grid it dispatches, and the
/// device buffer behind each storage binding, in binding order.
struct KernelPlan<'a> {
    name: &'a str,
    body: &'a Body,
    grid: [u32; 3],
    handles: Vec<DeviceHandleId>,
}

/// One compiled GPU entry point and its metadata.
#[derive(Debug)]
struct KernelArtifact {
    entry_point: String,
    grid_size: [u32; 3],
    /// Unrounded logical iteration extent (a 2-D/3-D `forall`'s loop lengths);
    /// lets a paint-writing kernel declare a rectangular canvas.
    logical_extent: Option<[u32; 3]>,
    wgsl_source: String,
    bindings: Vec<BufferBinding>,
    is_frame_step: bool,
    /// WGSL-line → Miri-line map, empty when the source was unavailable.
    source_map: Vec<SourceMapEntry>,
}

/// Emit the web-gpu bundle to disk. Returns the path of the bundle directory.
/// The caller chooses `out_path`: it is treated as a directory to fill;
/// `None` falls back to a unique tempdir.
pub fn emit_bundle(
    mir_bodies: &[(String, Body)],
    out_path: Option<&PathBuf>,
    source: Option<&str>,
    gpu_buffer_inits: Option<&HashMap<Span, GpuBufferInit>>,
) -> Result<PathBuf, CompilerError> {
    let kernels = extract_kernels(mir_bodies);
    if kernels.is_empty() {
        return Err(CompilerError::Codegen(
            "--target web-gpu requires the program to declare at least one GPU kernel \
             (no GPU kernels were found in the source)"
                .to_string(),
        ));
    }

    let host = HostProgram::scan(mir_bodies);
    let plans = plan_kernels(&kernels, &host)?;
    let no_inits = HashMap::new();
    let buffers = BufferTable::build(
        plans.iter().flat_map(plan_bindings),
        &host,
        gpu_buffer_inits.unwrap_or(&no_inits),
        source,
    )?;

    let bundle_dir = resolve_bundle_dir(out_path)?;
    fs::create_dir_all(&bundle_dir)?;

    // Device-side helper functions (`fn` called from a kernel) are cloned as
    // GpuDevice bodies by the frontend. Each kernel module must carry them so
    // its calls resolve in the browser validator, exactly as the native kernel
    // registry does.
    let helpers: Vec<(&str, &Body)> = mir_bodies
        .iter()
        .filter(|(_, body)| matches!(body.execution_model, ExecutionModel::GpuDevice))
        .map(|(name, body)| (name.as_str(), body))
        .collect();

    let artifacts = compile_kernels(&plans, &helpers, &buffers, source)?;

    // Derive program name from output directory or use default
    let program_name = out_path
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "gpu_program".to_string());
    let manifest = build_manifest(&program_name, &artifacts, &buffers)?;
    let manifest_path = bundle_dir.join(format!("{}.json", program_name));
    let manifest_json = manifest
        .to_json()
        .map_err(|err| CompilerError::Codegen(format!("Failed to serialize manifest: {}", err)))?;
    fs::write(&manifest_path, &manifest_json)?;
    write_runtime_files(&bundle_dir, &program_name, &manifest_json, &manifest.canvas)?;

    Ok(bundle_dir)
}

/// Writes the runtime driver, the headless runner and the dev-preview page
/// beside the manifest.
fn write_runtime_files(
    bundle_dir: &std::path::Path,
    program_name: &str,
    manifest_json: &str,
    canvas: &CanvasSpec,
) -> Result<(), CompilerError> {
    // Copy miri-gpu.js runtime
    fs::write(bundle_dir.join(MIRI_GPU_JS_FILENAME), MIRI_GPU_JS)?;

    // Headless runner + ES-module marker: a WebGPU-capable JS runtime
    // (Deno/Node) can boot the bundle without a browser for a CI smoke run.
    fs::write(
        bundle_dir.join(MIRI_GPU_HEADLESS_JS_FILENAME),
        MIRI_GPU_HEADLESS_JS,
    )?;
    fs::write(bundle_dir.join(PACKAGE_JSON_FILENAME), PACKAGE_JSON)?;

    // Generate a self-contained index.html dev preview: inline the runtime and
    // the manifest so it runs from a `file://` double-click (ES-module import +
    // JSON fetch are blocked under file://). The separate `<name>.json` +
    // `miri-gpu.js` files above are the artifacts for website integration.
    let html_text = generate_index_html(
        program_name,
        MIRI_GPU_JS,
        manifest_json,
        canvas.width,
        canvas.height,
    );
    fs::write(bundle_dir.join(INDEX_HTML_FILENAME), html_text)?;
    Ok(())
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

/// Pairs each kernel with its launch: the grid it dispatches and the device
/// buffer behind each storage binding.
fn plan_kernels<'a>(
    kernels: &[(&'a str, &'a Body)],
    host: &HostProgram,
) -> Result<Vec<KernelPlan<'a>>, CompilerError> {
    kernels
        .iter()
        .map(|&(name, body)| {
            let site = host.launch_of(name, body)?;
            let grid = buffers::fixed_grid(name, body, resolve_grid_size(body), host, &site)?;
            buffers::refuse_scalar_inputs(body, is_frame_step_kernel(body), &site)?;
            let handles = site
                .handles
                .iter()
                .map(|handle| {
                    handle.ok_or_else(|| {
                        CompilerError::Codegen(format!(
                            "kernel {name} is launched on a buffer with no device handle"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let binding_count = buffers::storage_params(body).count();
            if handles.len() != binding_count {
                return Err(CompilerError::Codegen(format!(
                    "kernel {name} binds {binding_count} storage buffers but its launch passes {}",
                    handles.len()
                )));
            }
            Ok(KernelPlan {
                name,
                body,
                grid,
                handles,
            })
        })
        .collect()
}

/// Each storage binding of a planned kernel: its device buffer and the kernel
/// parameter it binds as.
fn plan_bindings<'p>(
    plan: &'p KernelPlan<'p>,
) -> impl Iterator<Item = (DeviceHandleId, &'p LocalDecl)> + 'p {
    plan.handles
        .iter()
        .copied()
        .zip(buffers::storage_params(plan.body).map(|(_, decl)| decl))
}

fn compile_kernels(
    plans: &[KernelPlan],
    helpers: &[(&str, &Body)],
    buffers: &BufferTable,
    source: Option<&str>,
) -> Result<Vec<KernelArtifact>, CompilerError> {
    let options = WgslOptions::default();
    let mut artifacts = Vec::with_capacity(plans.len());

    for plan in plans {
        // Emit every reachable helper alongside the kernel; an unused helper is
        // a harmless dead function in WGSL.
        let mut module_bodies: Vec<(&str, &Body)> = Vec::with_capacity(1 + helpers.len());
        module_bodies.extend_from_slice(helpers);
        module_bodies.push((plan.name, plan.body));
        let module = compile_module(&module_bodies, &options)
            .map_err(|err| CompilerError::Codegen(err.to_string()))?;

        artifacts.push(KernelArtifact {
            entry_point: plan.name.to_string(),
            grid_size: plan.grid,
            logical_extent: resolve_logical_extent(plan.body),
            wgsl_source: module.wgsl,
            bindings: extract_buffer_bindings(plan, buffers)?,
            is_frame_step: is_frame_step_kernel(plan.body),
            source_map: build_source_map(&module.source_map, source),
        });
    }

    Ok(artifacts)
}

/// Convert the backend's WGSL-line → Miri-byte-offset spans into WGSL-line →
/// Miri-line entries against the displayed source. Empty when the source is
/// unavailable (nothing to highlight against).
fn build_source_map(
    spans: &[crate::codegen::wgsl::WgslSourceSpan],
    source: Option<&str>,
) -> Vec<SourceMapEntry> {
    let source = match source {
        Some(src) => src,
        None => return Vec::new(),
    };
    spans
        .iter()
        .map(|span| SourceMapEntry {
            wgsl: span.wgsl_line,
            miri: miri_line_of_offset(source, span.miri_offset),
        })
        .collect()
}

/// 1-based Miri source line containing `offset`.
fn miri_line_of_offset(source: &str, offset: usize) -> u32 {
    let clamped = offset.min(source.len());
    source[..clamped].bytes().filter(|&b| b == b'\n').count() as u32 + 1
}

fn resolve_grid_size(body: &Body) -> Option<[u32; 3]> {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.grid_size,
        None => None,
    }
}

fn resolve_logical_extent(body: &Body) -> Option<[u32; 3]> {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.logical_extent,
        None => None,
    }
}

fn is_frame_step_kernel(body: &Body) -> bool {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.is_frame_step,
        None => false,
    }
}

fn extract_buffer_bindings(
    plan: &KernelPlan,
    buffers: &BufferTable,
) -> Result<Vec<BufferBinding>, CompilerError> {
    let body = plan.body;
    buffers::storage_params(body)
        .zip(&plan.handles)
        .map(|((local_idx, decl), &handle)| {
            let buffer = buffers.buffer_of(handle).ok_or_else(|| {
                CompilerError::Codegen(format!("device buffer {handle} has no bundle entry"))
            })?;
            // Atomic buffers need read-write access; check the element type
            let is_atomic_buffer = is_atomic_element_buffer(&decl.ty.kind);
            let is_out = body.out_params.get(local_idx - 1).copied().unwrap_or(false);
            let writes = body
                .param_written
                .get(local_idx - 1)
                .copied()
                .unwrap_or(is_out);
            Ok(BufferBinding {
                name: buffer.name.clone(),
                element_type: buffer.element_type,
                read_only: !is_atomic_buffer && !is_out,
                writes,
            })
        })
        .collect()
}

fn build_manifest(
    program_name: &str,
    artifacts: &[KernelArtifact],
    buffers: &BufferTable,
) -> Result<Manifest, CompilerError> {
    // `BufferTable` iterates by name, producing byte-identical bundles from
    // identical source.
    let buffer_specs: Vec<BufferSpec> = buffers
        .iter()
        .map(|buffer| {
            // A sized constructor is zero-filled by the runtime: no initialData.
            let initial_data_json = if buffer.is_zero_filled || buffer.initial_data.is_empty() {
                None
            } else {
                Some(
                    buffer
                        .initial_data
                        .iter()
                        .map(|v| {
                            if v.fract() == 0.0 {
                                json!(*v as i64)
                            } else {
                                json!(v)
                            }
                        })
                        .collect(),
                )
            };
            BufferSpec {
                name: buffer.name.clone(),
                elem_type: buffer.element_type.to_string(),
                length: buffer.length as u32,
                initial_data: initial_data_json,
            }
        })
        .collect();

    // Compute canvas dimensions from paint buffer. The display target is a
    // writable buffer of the last relevant kernel — preferring an `f32` one, so
    // an atomic scratch buffer (bound read_write for accumulation, e.g. a
    // particle density surface) never shadows the real RGBA paint output.
    let paint_buffer = artifacts
        .iter()
        .rev()
        .find(|a| a.is_frame_step)
        .and_then(|a| paint_binding(a.bindings.iter()))
        .map(|b| b.name.clone())
        .or_else(|| {
            // Static demo: paint the output of the LAST kernel in the pipeline
            // (e.g. box-blur's `dst`, not the seed kernel's `src`).
            artifacts
                .last()
                .and_then(|a| paint_binding(a.bindings.iter().rev()))
                .map(|b| b.name.clone())
        })
        .unwrap_or_else(|| "output".to_string());

    let paint_length = buffers
        .get(&paint_buffer)
        .map(|buffer| buffer.length)
        .unwrap_or(4096);

    // Infer paint_mode BEFORE computing canvas dimensions.
    // Check if the paint buffer is f32 with length = 4 * pixel_count.
    // If so, it's RGBA; otherwise it's colormap.
    let (paint_mode, effective_paint_length) = buffers
        .get(&paint_buffer)
        .map(|buffer| {
            let len = buffer.length;
            if buffer.element_type == "f32" && len % 4 == 0 {
                // RGBA mode: length is 4 * pixel_count
                ("rgba".to_string(), len / 4)
            } else {
                // Colormap mode: length is pixel_count
                ("colormap".to_string(), len)
            }
        })
        .unwrap_or_else(|| ("colormap".to_string(), paint_length));

    // Prefer an explicit rectangular canvas: a 2-D `forall` that writes the
    // paint buffer declares its exact (width, height). Frame paint passes are
    // 1-D, so a demo conveys a non-square canvas via a 2-D kernel writing paint
    // (e.g. a seed that clears it). Fall back to the square inference from the
    // paint pixel count when no such kernel exists (the common square demo).
    let (canvas_width, canvas_height) = paint_canvas_extent(artifacts, &paint_buffer)
        .unwrap_or_else(|| compute_canvas_dimensions(effective_paint_length));

    let paint_mode = if paint_mode == "rgba" {
        Some(paint_mode)
    } else {
        None
    };

    // Split kernels into seed and frame passes
    let mut seed_kernels = Vec::new();
    let mut frame_passes = Vec::new();

    for artifact in artifacts {
        let kernel_spec = build_kernel_spec(artifact)?;
        if artifact.is_frame_step {
            frame_passes.push(kernel_spec);
        } else {
            seed_kernels.push(kernel_spec);
        }
    }

    Ok(Manifest {
        name: program_name.to_string(),
        canvas: CanvasSpec {
            width: canvas_width,
            height: canvas_height,
        },
        buffers: buffer_specs,
        seed: seed_kernels,
        frame_passes,
        paint: paint_buffer,
        paint_mode,
    })
}

fn build_kernel_spec(artifact: &KernelArtifact) -> Result<KernelSpec, CompilerError> {
    let bindings = artifact
        .bindings
        .iter()
        .map(|b| BindingSpec {
            name: b.name.clone(),
            access: if b.read_only {
                "read".to_string()
            } else {
                "read_write".to_string()
            },
            writes: b.writes,
        })
        .collect();

    // For frame kernels, identify read and write buffers. These key on the
    // pass's real data flow rather than the storage qualifier, so an atomic
    // buffer the pass only reads is not reported as the written one.
    let (read, write) = if artifact.is_frame_step {
        let read_buf = artifact
            .bindings
            .iter()
            .find(|b| !b.writes)
            .map(|b| b.name.clone());
        let write_buf = artifact
            .bindings
            .iter()
            .find(|b| b.writes)
            .map(|b| b.name.clone());
        (read_buf, write_buf)
    } else {
        (None, None)
    };

    // For frame kernels, populate the frame input fields
    let inputs = if artifact.is_frame_step {
        Some(build_frame_inputs())
    } else {
        None
    };

    // Planning refused every kernel whose grid is known only at run time.
    let workgroups = artifact.grid_size;

    Ok(KernelSpec {
        entry_point: artifact.entry_point.clone(),
        wgsl: artifact.wgsl_source.clone(),
        workgroups,
        bindings,
        read,
        write,
        inputs,
        source_map: artifact.source_map.clone(),
    })
}

fn build_frame_inputs() -> Vec<InputFieldSpec> {
    FRAME_INPUT_FIELDS
        .iter()
        .enumerate()
        .map(|(idx, def)| {
            let ty = match def.kind {
                FrameFieldKind::F32 => "f32".to_string(),
                FrameFieldKind::Int => "i32".to_string(),
                FrameFieldKind::Bool => "u32".to_string(),
            };
            let offset = (idx as u32) * 4;
            InputFieldSpec {
                name: def.name.to_string(),
                ty,
                offset,
            }
        })
        .collect()
}

/// The paint (display) binding among a kernel's bindings: the first writable
/// `f32` buffer (the RGBA/scalar display target), or — if none is `f32` — the
/// first writable buffer. Preferring `f32` keeps an atomic `u32`/`i32` scratch
/// buffer (bound read_write for accumulation) from being mistaken for paint.
fn paint_binding<'a>(
    bindings: impl Iterator<Item = &'a BufferBinding>,
) -> Option<&'a BufferBinding> {
    let mut first_writable = None;
    for b in bindings {
        if b.read_only {
            continue;
        }
        if b.element_type == "f32" {
            return Some(b);
        }
        first_writable.get_or_insert(b);
    }
    first_writable
}

/// The rectangular canvas declared by a 2-D (or 3-D) `forall` that writes the
/// paint buffer: its unrounded logical (width, height). `None` when no such
/// kernel exists, or when the only paint writer is 1-D (height == 1) — those
/// fall back to the square inference from pixel count.
fn paint_canvas_extent(artifacts: &[KernelArtifact], paint_buffer: &str) -> Option<(u32, u32)> {
    artifacts
        .iter()
        .filter(|a| {
            a.bindings
                .iter()
                .any(|b| b.name == paint_buffer && !b.read_only)
        })
        .find_map(|a| match a.logical_extent {
            Some([w, h, _]) if h > 1 => Some((w, h)),
            _ => None,
        })
}

fn compute_canvas_dimensions(length: usize) -> (u32, u32) {
    let sqrt = (length as f64).sqrt().floor() as u32;
    if sqrt * sqrt == length as u32 {
        (sqrt, sqrt)
    } else {
        (length as u32, 1)
    }
}

fn generate_index_html(
    program_name: &str,
    runtime_js: &str,
    manifest_json: &str,
    canvas_width: u32,
    canvas_height: u32,
) -> String {
    // Escape `</` so an embedded WGSL/JSON string can never close the <script>.
    let manifest_inline = manifest_json.replace("</", "<\\/");
    let runtime_inline = runtime_js.replace("</", "<\\/");
    // The display frame matches the compute grid's aspect so a non-square demo
    // (e.g. a 16:9 grid) is not letterboxed or stretched.
    let aspect_w = canvas_width.max(1);
    let aspect_h = canvas_height.max(1);

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{name} — Miri GPU</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500&family=Space+Grotesk:wght@400;500;600;700&display=swap" rel="stylesheet" />
    <style>
        :root {{
            --bg: #04070f; --panel: #0a1326; --line: rgba(110, 142, 255, 0.13);
            --text: #e9eefb; --muted: #93a3c9; --dim: #5b6b95;
            --yellow: #ffd83d; --blue: #5b8cff; --radius-lg: 16px;
            --font-display: "Space Grotesk", system-ui, sans-serif;
            --font-mono: "JetBrains Mono", ui-monospace, "SF Mono", monospace;
        }}
        * {{ box-sizing: border-box; }}
        body {{
            background: var(--bg); color: var(--text); font-family: var(--font-display);
            font-size: 17px; line-height: 1.6; margin: 0; padding: 3rem 1.5rem;
            -webkit-font-smoothing: antialiased; display: flex; flex-direction: column; align-items: center;
        }}
        .wrap {{ width: 100%; max-width: min(96vw, 960px); }}
        h1 {{ font-weight: 700; font-size: 2rem; margin: 0 0 0.35rem; letter-spacing: -0.02em; }}
        p.lead {{ color: var(--muted); margin: 0 0 1.75rem; }}
        .stage {{
            border: 1px solid var(--line); border-radius: var(--radius-lg); overflow: hidden;
            background: var(--panel); box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
        }}
        .frame {{
            position: relative; background: #02040a; width: 100%; aspect-ratio: {aspect_w} / {aspect_h};
        }}
        .frame canvas {{
            position: absolute; inset: 0; width: 100%; height: 100%; display: block;
            image-rendering: auto; touch-action: none; cursor: grab;
        }}
        .frame canvas:active {{ cursor: grabbing; }}
        .controls {{
            display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap;
            gap: 10px 16px; padding: 12px 18px; border-top: 1px solid var(--line);
            font-family: var(--font-mono); font-size: 12px; color: var(--dim);
            background: rgba(7, 13, 29, 0.7);
        }}
        .hint {{ color: var(--muted); }}
        .hint::before {{ content: "✦ "; color: var(--yellow); }}
        #fps b {{ color: var(--text); font-weight: 500; }}
        #fps.fail {{ color: #ff6b6b; }}
    </style>
</head>
<body>
    <div class="wrap">
        <h1>{name}</h1>
        <div class="stage">
            <div class="frame">
                <canvas id="output" width="64" height="64" aria-label="Compute output"></canvas>
            </div>
            <div class="controls">
                <span class="hint">drag to pan · scroll to zoom</span>
                <span id="fps">fps <b>—</b></span>
            </div>
        </div>
    </div>

    <script type="module">
// --- inlined miri-gpu.js runtime (self-contained for file:// preview) ---
{runtime}
// --- end runtime ---

        const canvas = document.getElementById("output");
        const fpsEl = document.getElementById("fps");
        const MANIFEST = {manifest};

        // Rolling FPS: count painted frames and refresh the readout ~2x/second.
        let frames = 0;
        let windowStart = performance.now();
        function onFrame() {{
            frames++;
            const now = performance.now();
            const elapsed = now - windowStart;
            if (elapsed >= 500) {{
                const fps = Math.round((frames * 1000) / elapsed);
                fpsEl.innerHTML = `fps <b>${{fps}}</b>`;
                frames = 0;
                windowStart = now;
            }}
        }}

        (async () => {{
            try {{
                await mount(canvas, MANIFEST, {{ powerPreference: "high-performance", onFrame }});
            }} catch (err) {{
                fpsEl.textContent = `error: ${{err.message ?? err}}`;
                fpsEl.className = "fail";
            }}
        }})();
    </script>
</body>
</html>
"##,
        name = program_name,
        runtime = runtime_inline,
        manifest = manifest_inline,
    )
}
