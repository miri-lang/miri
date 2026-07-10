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
use crate::codegen::wgsl::{compile_module, WgslOptions};
use crate::error::compiler::CompilerError;
use crate::mir::backend::BackendMetadata;
use crate::mir::{Body, ExecutionModel};
use crate::type_checker::GpuBufferInit;
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

    let artifacts = compile_kernels(&kernels, &helpers, gpu_buffer_inits, source)?;

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
    let index_path = bundle_dir.join(INDEX_HTML_FILENAME);
    let html_text = generate_index_html(
        &program_name,
        MIRI_GPU_JS,
        &manifest_json,
        manifest.canvas.width,
        manifest.canvas.height,
    );
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
    source: Option<&str>,
) -> Result<Vec<KernelArtifact>, CompilerError> {
    let options = WgslOptions::default();
    let mut artifacts = Vec::with_capacity(kernels.len());

    for (name, body) in kernels {
        // Emit every reachable helper alongside the kernel; an unused helper is
        // a harmless dead function in WGSL.
        let mut module_bodies: Vec<(&str, &Body)> = Vec::with_capacity(1 + helpers.len());
        module_bodies.extend_from_slice(helpers);
        module_bodies.push((name.as_str(), body));
        let module = compile_module(&module_bodies, &options)
            .map_err(|err| CompilerError::Codegen(err.to_string()))?;

        let bindings = extract_buffer_bindings(body, gpu_buffer_inits);
        let is_frame_step = is_frame_step_kernel(body);
        let grid_size = resolve_grid_size(body);
        let logical_extent = resolve_logical_extent(body);
        let source_map = build_source_map(&module.source_map, source);

        artifacts.push(KernelArtifact {
            entry_point: name.clone(),
            grid_size,
            logical_extent,
            wgsl_source: module.wgsl,
            bindings,
            is_frame_step,
            source_map,
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
            TypeKind::F16 => Some("f16"),
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
        | TypeKind::F16
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

    // Convert to BufferSpec list. `all_buffers` is a HashMap, so its iteration
    // order is nondeterministic; sort by the unique buffer name below so the
    // emitted manifest is reproducible (identical source → identical bundle).
    let mut buffers: Vec<BufferSpec> = all_buffers
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
    buffers.sort_by(|a, b| a.name.cmp(&b.name));

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
        <p class="lead">GPU-accelerated computation, compiled from Miri to WebGPU.</p>
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
