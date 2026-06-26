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

mod manifest;

use crate::ast::types::{FrameFieldKind, TypeKind, FRAME_INPUT_FIELDS};
use crate::codegen::wgsl::{WgslBackend, WgslOptions};
use crate::codegen::Backend;
use crate::error::compiler::CompilerError;
use crate::mir::backend::BackendMetadata;
use crate::mir::{Body, ExecutionModel};
use manifest::{BindingSpec, BufferSpec, CanvasSpec, InputFieldSpec, KernelSpec, Manifest};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const MIRI_GPU_JS: &str = include_str!("../../../assets/web/miri-gpu.js");
const MIRI_GPU_JS_FILENAME: &str = "miri-gpu.js";
const INDEX_HTML_FILENAME: &str = "index.html";

/// Initial data for a GPU buffer from a compile-time constant initializer.
#[derive(Debug, Clone)]
pub struct GpuBufferInit {
    pub elem_type: String,
    pub values: Vec<f64>,
    pub length: Option<usize>, // Explicit length for sized allocations; None means infer from values.len()
}

/// Per-binding metadata for a kernel's storage buffer.
#[derive(Debug, Clone)]
pub(crate) struct BufferBinding {
    pub name: String,
    pub element_type: String,
    pub length: usize,
    pub read_only: bool,
    pub initial_data: Vec<f64>,
    /// True if this buffer was zero-filled (sized-ctor like Array<T, N>()).
    /// When true, initialData should be null in the manifest.
    pub is_zero_filled: bool,
}

/// One compiled GPU entry point and its metadata.
#[derive(Debug)]
struct KernelArtifact {
    entry_point: String,
    grid_size: Option<[u32; 3]>,
    wgsl_source: String,
    bindings: Vec<BufferBinding>,
    is_frame_step: bool,
}

/// Emit the web-gpu bundle to disk. Returns the path of the bundle directory.
/// The caller chooses `out_path`: it is treated as a directory to fill;
/// `None` falls back to a unique tempdir.
pub fn emit_bundle(
    mir_bodies: &[(String, Body)],
    out_path: Option<&PathBuf>,
    source: Option<&str>,
    gpu_buffer_inits: Option<&HashMap<String, GpuBufferInit>>,
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

    // Device-side helper functions (`fn` called from a kernel) are cloned as
    // GpuDevice bodies by the frontend. Each kernel module must carry them so
    // its calls resolve in the browser validator, exactly as the native kernel
    // registry does.
    let helpers: Vec<(&str, &Body)> = mir_bodies
        .iter()
        .filter(|(_, body)| matches!(body.execution_model, ExecutionModel::GpuDevice))
        .map(|(name, body)| (name.as_str(), body))
        .collect();

    let artifacts = compile_kernels(&kernels, &helpers, gpu_buffer_inits)?;

    // Derive program name from output directory or use default
    let program_name = out_path
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "gpu_program".to_string());
    let manifest = build_manifest(&program_name, &artifacts, gpu_buffer_inits)?;
    let manifest_path = bundle_dir.join(format!("{}.json", program_name));
    let manifest_json = manifest
        .to_json()
        .map_err(|err| CompilerError::Codegen(format!("Failed to serialize manifest: {}", err)))?;
    fs::write(&manifest_path, &manifest_json)?;

    // Copy miri-gpu.js runtime
    fs::write(bundle_dir.join(MIRI_GPU_JS_FILENAME), MIRI_GPU_JS)?;

    // Generate a self-contained index.html dev preview: inline the runtime and
    // the manifest so it runs from a `file://` double-click (ES-module import +
    // JSON fetch are blocked under file://). The separate `<name>.json` +
    // `miri-gpu.js` files above are the artifacts for website integration.
    let index_path = bundle_dir.join(INDEX_HTML_FILENAME);
    let html_text = generate_index_html(&program_name, source, MIRI_GPU_JS, &manifest_json);
    fs::write(&index_path, html_text)?;

    Ok(bundle_dir)
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

fn extract_kernels(mir_bodies: &[(String, Body)]) -> Vec<(String, Body)> {
    mir_bodies
        .iter()
        .filter(|(_, body)| matches!(body.execution_model, ExecutionModel::GpuKernel))
        .map(|(name, body)| (name.clone(), body.clone()))
        .collect()
}

fn compile_kernels(
    kernels: &[(String, Body)],
    helpers: &[(&str, &Body)],
    gpu_buffer_inits: Option<&HashMap<String, GpuBufferInit>>,
) -> Result<Vec<KernelArtifact>, CompilerError> {
    let backend = WgslBackend;
    let options = WgslOptions::default();
    let mut artifacts = Vec::with_capacity(kernels.len());

    for (name, body) in kernels {
        // Emit every reachable helper alongside the kernel; an unused helper is
        // a harmless dead function in WGSL.
        let mut module_bodies: Vec<(&str, &Body)> = Vec::with_capacity(1 + helpers.len());
        module_bodies.extend_from_slice(helpers);
        module_bodies.push((name.as_str(), body));
        let artifact = backend
            .compile(&module_bodies, &options)
            .map_err(|err| CompilerError::Codegen(err.to_string()))?;
        let wgsl_text = String::from_utf8(artifact.bytes).map_err(|err| {
            CompilerError::Codegen(format!(
                "WGSL backend produced non-UTF-8 output for kernel '{}': {}",
                name, err
            ))
        })?;

        let bindings = extract_buffer_bindings(body, gpu_buffer_inits);
        let is_frame_step = is_frame_step_kernel(body);
        let grid_size = resolve_grid_size(body);

        artifacts.push(KernelArtifact {
            entry_point: name.clone(),
            grid_size,
            wgsl_source: wgsl_text,
            bindings,
            is_frame_step,
        });
    }

    Ok(artifacts)
}

fn resolve_grid_size(body: &Body) -> Option<[u32; 3]> {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.grid_size,
        None => None,
    }
}

fn is_frame_step_kernel(body: &Body) -> bool {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(gpu)) => gpu.is_frame_step,
        None => false,
    }
}

/// Extract the WGSL element type string from a buffer (Array/List) parameter type.
///
/// Returns the WGSL type name ("i32", "f32", etc.) for the buffer's element type.
/// Falls back to "i32" if the type cannot be resolved.
fn buffer_element_type_string(param_ty: &TypeKind) -> String {
    use crate::ast::types::BuiltinCollectionKind;

    fn scalar_name(kind: &TypeKind) -> Option<&'static str> {
        match kind {
            TypeKind::I32 | TypeKind::I8 | TypeKind::I16 => Some("i32"),
            TypeKind::U32 | TypeKind::U8 | TypeKind::U16 => Some("u32"),
            TypeKind::F32 => Some("f32"),
            TypeKind::Boolean => Some("bool"),
            TypeKind::Int => Some("i32"),
            TypeKind::I64 => Some("i64"),
            TypeKind::U64 => Some("u64"),
            TypeKind::Float | TypeKind::F64 => Some("f64"),
            TypeKind::I128
            | TypeKind::U128
            | TypeKind::String
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Custom(_, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Linear(_) => None,
        }
    }

    match param_ty {
        TypeKind::Array(elem_expr, _) | TypeKind::List(elem_expr) => {
            if let crate::ast::expression::ExpressionKind::Type(inner, _) = &elem_expr.node {
                scalar_name(&inner.kind)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "i32".to_string())
            } else {
                "i32".to_string()
            }
        }
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array) | Some(BuiltinCollectionKind::List)
            ) =>
        {
            if let Some(elem_expr) = args.first() {
                if let crate::ast::expression::ExpressionKind::Type(inner, _) = &elem_expr.node {
                    scalar_name(&inner.kind)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "i32".to_string())
                } else {
                    "i32".to_string()
                }
            } else {
                "i32".to_string()
            }
        }
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => "i32".to_string(),
    }
}

/// Check if a buffer has Atomic element types and therefore needs read-write access.
fn is_buffer_atomic_element(param_ty: &TypeKind) -> bool {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::types::BuiltinCollectionKind;

    match param_ty {
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array) | Some(BuiltinCollectionKind::List)
            ) =>
        {
            if let Some(elem_expr) = args.first() {
                if let ExpressionKind::Type(inner, _) = &elem_expr.node {
                    if let TypeKind::Custom(elem_name, Some(inner_args)) = &inner.kind {
                        return elem_name == crate::ast::types::ATOMIC_TYPE_NAME
                            && !inner_args.is_empty();
                    }
                }
            }
            false
        }
        _ => false,
    }
}

fn extract_buffer_bindings(
    body: &Body,
    gpu_buffer_inits: Option<&HashMap<String, GpuBufferInit>>,
) -> Vec<BufferBinding> {
    let mut bindings = Vec::new();

    for param_idx in 1..=body.arg_count {
        let decl = match body.local_decls.get(param_idx) {
            Some(d) => d,
            None => continue,
        };

        let is_storage_buffer = matches!(
            decl.storage_class,
            crate::mir::body::StorageClass::GpuGlobal
                | crate::mir::body::StorageClass::StorageBuffer
        );

        if !is_storage_buffer {
            continue;
        }

        // Atomic buffers need read-write access; check the element type
        let is_atomic_buffer = is_buffer_atomic_element(&decl.ty.kind);
        let read_only =
            !is_atomic_buffer && !body.out_params.get(param_idx - 1).copied().unwrap_or(false);

        let name = decl
            .name
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("_buf{}", param_idx));

        let (element_type, length, initial_data, is_zero_filled) =
            if let Some(inits) = gpu_buffer_inits {
                if let Some(init) = inits.get(&name) {
                    let is_sized = init.length.is_some();
                    (
                        init.elem_type.clone(),
                        init.length.unwrap_or(init.values.len()),
                        init.values.clone(),
                        is_sized, // Zero-filled if explicitly sized (Array<T, N>())
                    )
                } else {
                    let elem_type = buffer_element_type_string(&decl.ty.kind);
                    (elem_type, 0, Vec::new(), false)
                }
            } else {
                let elem_type = buffer_element_type_string(&decl.ty.kind);
                (elem_type, 0, Vec::new(), false)
            };

        bindings.push(BufferBinding {
            name,
            element_type,
            length,
            read_only,
            initial_data,
            is_zero_filled,
        });
    }

    bindings
}

fn build_manifest(
    program_name: &str,
    artifacts: &[KernelArtifact],
    _gpu_buffer_inits: Option<&HashMap<String, GpuBufferInit>>,
) -> Result<Manifest, CompilerError> {
    // Collect all unique buffers with their metadata
    let all_buffers: HashMap<String, (String, usize, Vec<f64>, bool)> = {
        let mut buffers = HashMap::new();
        for artifact in artifacts {
            for binding in &artifact.bindings {
                buffers.insert(
                    binding.name.clone(),
                    (
                        binding.element_type.clone(),
                        binding.length,
                        binding.initial_data.clone(),
                        binding.is_zero_filled,
                    ),
                );
            }
        }
        buffers
    };

    // Convert to BufferSpec list
    let buffers: Vec<BufferSpec> = all_buffers
        .iter()
        .map(
            |(name, (elem_type, length, initial_data, is_zero_filled))| {
                // Emit initialData for every buffer:
                // - If zero-filled (sized-ctor), emit null
                // - If has literal data, emit the values
                // - If empty (uninitialized), emit null
                let initial_data_json = if *is_zero_filled || initial_data.is_empty() {
                    None
                } else {
                    Some(
                        initial_data
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
                    name: name.clone(),
                    elem_type: elem_type.clone(),
                    length: *length as u32,
                    initial_data: initial_data_json,
                }
            },
        )
        .collect();

    // Compute canvas dimensions from paint buffer
    let paint_buffer = artifacts
        .iter()
        .rev()
        .find(|a| a.is_frame_step)
        .and_then(|a| a.bindings.iter().find(|b| !b.read_only))
        .map(|b| b.name.clone())
        .or_else(|| {
            // Static demo: paint the output of the LAST kernel in the pipeline
            // (e.g. box-blur's `dst`, not the seed kernel's `src`).
            artifacts
                .last()
                .and_then(|a| a.bindings.iter().rev().find(|b| !b.read_only))
                .map(|b| b.name.clone())
        })
        .unwrap_or_else(|| "output".to_string());

    let paint_length = all_buffers
        .get(&paint_buffer)
        .map(|(_, len, _, _)| *len)
        .unwrap_or(4096);

    // Infer paint_mode BEFORE computing canvas dimensions.
    // Check if the paint buffer is f32 with length = 4 * pixel_count.
    // If so, it's RGBA; otherwise it's colormap.
    let (paint_mode, effective_paint_length) = all_buffers
        .get(&paint_buffer)
        .map(|(elem_type, len, _, _)| {
            if elem_type == "f32" && *len % 4 == 0 {
                // RGBA mode: length is 4 * pixel_count
                ("rgba".to_string(), *len / 4)
            } else {
                // Colormap mode: length is pixel_count
                ("colormap".to_string(), *len)
            }
        })
        .unwrap_or_else(|| ("colormap".to_string(), paint_length));

    let (canvas_width, canvas_height) = compute_canvas_dimensions(effective_paint_length);

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
        buffers,
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
        })
        .collect();

    // For frame kernels, identify read and write buffers
    let (read, write) = if artifact.is_frame_step {
        let read_buf = artifact
            .bindings
            .iter()
            .find(|b| b.read_only)
            .map(|b| b.name.clone());
        let write_buf = artifact
            .bindings
            .iter()
            .find(|b| !b.read_only)
            .map(|b| b.name.clone());
        (read_buf, write_buf)
    } else {
        (None, None)
    };

    // For frame kernels, populate the 11 frame input fields
    let inputs = if artifact.is_frame_step {
        Some(build_frame_inputs())
    } else {
        None
    };

    // Use grid_size (dispatch grid) if available; fallback to a default grid of [1,1,1]
    // for runtime-bound kernels where grid is computed at runtime.
    let workgroups = artifact.grid_size.unwrap_or([1, 1, 1]);

    Ok(KernelSpec {
        entry_point: artifact.entry_point.clone(),
        wgsl: artifact.wgsl_source.clone(),
        workgroups,
        bindings,
        read,
        write,
        inputs,
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
    source: Option<&str>,
    runtime_js: &str,
    manifest_json: &str,
) -> String {
    // Escape `</` so an embedded WGSL/JSON string can never close the <script>.
    let manifest_inline = manifest_json.replace("</", "<\\/");
    let runtime_inline = runtime_js.replace("</", "<\\/");
    let source_panel = source
        .map(|src| {
            let escaped = escape_html(src);
            format!(
                r#"<div id="sourcePanel" style="margin-top: 2rem; padding: 1rem; background: #f9f9f9; border: 1px solid #ddd; border-radius: 4px;">
    <h2 style="margin-top: 0;">Source Code</h2>
    <pre style="background: #fff; padding: 0.75rem; border: 1px solid #e0e0e0; border-radius: 3px; overflow-x: auto; font-size: 0.85rem;">{}</pre>
</div>"#,
                escaped
            )
        })
        .unwrap_or_default();

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <title>{} - Miri GPU</title>
    <style>
        body {{ font-family: -apple-system, system-ui, sans-serif; max-width: 64rem; margin: 2rem auto; padding: 0 1rem; }}
        h1 {{ margin-bottom: 0.25rem; }}
        #layout {{ display: grid; grid-template-columns: 1fr 1fr; gap: 2rem; margin: 1rem 0; }}
        #renderPanel {{ }}
        #sourcePanel {{ background: #f9f9f9; border: 1px solid #ddd; border-radius: 4px; padding: 1rem; }}
        canvas {{ display: block; margin: 1rem 0; background: #111; image-rendering: pixelated; width: 256px; height: 256px; }}
        pre {{ background: #fff; padding: 0.75rem; border: 1px solid #e0e0e0; border-radius: 3px; overflow-x: auto; font-size: 0.85rem; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; }}
        #log {{ font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.85rem; background: #f5f5f5; border: 1px solid #ddd; border-radius: 4px; padding: 0.75rem; white-space: pre-wrap; max-height: 300px; overflow-y: auto; }}
        .pass {{ color: #0a7d28; font-weight: 600; }}
        .fail {{ color: #b00020; font-weight: 600; }}
        @media (max-width: 900px) {{
            #layout {{ grid-template-columns: 1fr; }}
        }}
    </style>
</head>
<body>
    <h1>{} Demo</h1>
    <p>GPU-accelerated computation rendered from a manifest. Open in a WebGPU-capable browser.</p>
    <div id="layout">
        <div id="renderPanel">
            <label>Colormap:
                <select id="colormap">
                    <option value="grayscale">grayscale</option>
                    <option value="spectrum">spectrum</option>
                    <option value="fire">fire</option>
                </select>
            </label>
            <canvas id="output" width="64" height="64" aria-label="Compute output pixel grid"></canvas>
            <div id="status">Loading manifest...</div>
            <pre id="log"></pre>
        </div>
        <div>
            {}
        </div>
    </div>

    <script type="module">
// --- inlined miri-gpu.js runtime (self-contained for file:// preview) ---
{}
// --- end runtime ---

        const status = document.getElementById("status");
        const log = document.getElementById("log");
        const canvas = document.getElementById("output");
        const colormapSelect = document.getElementById("colormap");
        const MANIFEST = {};
        let handle = null;

        function logLine(msg) {{
            log.textContent += msg + "\n";
            log.scrollTop = log.scrollHeight;
        }}

        async function runDemo() {{
            try {{
                if (handle) handle.stop();
                handle = await mount(canvas, MANIFEST, {{
                    powerPreference: "high-performance",
                    colormap: colormapSelect.value,
                }});
                status.textContent = "Running...";
                status.className = "pass";
            }} catch (err) {{
                logLine(`Error: ${{err.message ?? err}}`);
                status.textContent = "Failed";
                status.className = "fail";
            }}
        }}

        colormapSelect.addEventListener("change", runDemo);
        runDemo();
    </script>
</body>
</html>
"##,
        program_name, program_name, source_panel, runtime_inline, manifest_inline
    )
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}
