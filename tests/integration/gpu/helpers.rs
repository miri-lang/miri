// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Helpers for GPU integration tests: compile Miri source to WGSL, validate
//! it with `naga`, and dispatch it through `wgpu` on the host.
//!
//! These helpers stage everything wgpu needs (bind groups, storage buffers,
//! readback, grid calculation) from the test harness, bypassing the
//! Cranelift `miri_gpu_launch_inline` path. They exist because the native
//! dispatch path has a documented `int → i32` width mismatch — see
//! `super::launch` and PLAN M6.5 task "WGSL int-width fix". Once that lands,
//! these helpers shrink to shader-level naga validation only (PLAN M6.5 task
//! "Helper-shrink").

use std::sync::mpsc;

use miri::ast::statement::StatementKind;
use miri::codegen::backend::Backend;
use miri::codegen::wgsl::{WgslBackend, WgslOptions};
use miri::mir::lowering::lower_function;
use miri::mir::ExecutionModel;
use miri::pipeline::Pipeline;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, BufferBindingType, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipelineDescriptor, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, Limits, Maintain, MapMode, PipelineLayoutDescriptor, PowerPreference,
    RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, ShaderStages,
};

/// Result of running the frontend, lowering, and WGSL backend on a Miri
/// source: the kernel's WGSL text and the synthesized entry-point name.
pub struct CompiledKernel {
    pub wgsl: String,
    pub entry_point: String,
}

/// Compile a Miri source that contains a `gpu for` loop or a `gpu fn` to
/// WGSL. Returns the first `GpuKernel` body produced by lowering.
pub fn compile_to_wgsl(source: &str) -> CompiledKernel {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(
            |stmt| matches!(&stmt.node, StatementKind::FunctionDeclaration(d) if d.name == "main"),
        )
        .expect("source must contain 'fn main'");

    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering failed");

    let kernel = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("expected a synthesized GpuKernel body");

    let artifact = WgslBackend
        .compile(
            &[(kernel.name.as_str(), &kernel.body)],
            &WgslOptions::default(),
        )
        .expect("WGSL backend should succeed");
    let wgsl = String::from_utf8(artifact.bytes).expect("WGSL output is UTF-8");
    CompiledKernel {
        wgsl,
        entry_point: kernel.name.clone(),
    }
}

/// Parse and validate WGSL text with `naga` — runs at every call site so
/// invalid output is rejected before any `wgpu` dispatch attempt.
pub fn naga_validate(source: &str) {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|err| {
        panic!(
            "naga failed to parse generated WGSL:\n{}\n--- source ---\n{}",
            err.emit_to_string(source),
            source
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).unwrap_or_else(|err| {
        panic!(
            "naga failed to validate generated WGSL: {:?}\n--- source ---\n{}",
            err, source
        )
    });
}

/// Compile a Miri source to WGSL and validate it with `naga`. Use for
/// hardware-free shader-correctness tests.
pub fn assert_gpu_wgsl_valid(source: &str) {
    let CompiledKernel { wgsl, .. } = compile_to_wgsl(source);
    naga_validate(&wgsl);
}

/// Compile a Miri source, validate it with `naga`, then dispatch the
/// kernel through `wgpu` and compare every storage binding against
/// `expected`. `inputs[i]` seeds binding `i`; `expected[i]` is the
/// post-dispatch contents to assert.
///
/// Bindings appear in capture-discovery order (see
/// `src/mir/lowering/gpu_for.rs::collect_outer_captures`): the first outer
/// identifier mentioned in the kernel body is binding 0, and so on. Tests
/// must order `inputs` / `expected` accordingly.
///
/// Dispatches with `dispatch_workgroups(1, 1, 1)` against the synthesized
/// `@workgroup_size(256, 1, 1)` kernel, so each test covers up to 256
/// elements per binding. If `wgpu` cannot acquire an adapter that supports
/// `Features::SHADER_INT64` (no GPU, no fallback, or 64-bit integers
/// unsupported on this device) the call returns without asserting, keeping
/// the suite green where the new WGSL `i64` mapping cannot be exercised.
pub fn assert_gpu_compute_i64(source: &str, inputs: &[&[i64]], expected: &[&[i64]]) {
    assert_eq!(
        inputs.len(),
        expected.len(),
        "inputs and expected must list the same number of bindings"
    );
    let CompiledKernel { wgsl, entry_point } = compile_to_wgsl(source);
    naga_validate(&wgsl);

    let Some(device_ctx) = try_init_wgpu() else {
        eprintln!(
            "[gpu_wgsl] skipped: no wgpu adapter with SHADER_INT64 (entry_point = {})",
            entry_point
        );
        return;
    };

    let inputs_bytes: Vec<Vec<u8>> = inputs.iter().map(|s| i64_slice_to_bytes(s)).collect();
    let inputs_byte_refs: Vec<&[u8]> = inputs_bytes.iter().map(Vec::as_slice).collect();
    let outputs_bytes = dispatch_compute(&device_ctx, &wgsl, &entry_point, &inputs_byte_refs);
    for (i, (got, want)) in outputs_bytes.iter().zip(expected.iter()).enumerate() {
        let got_i64 = bytes_to_i64_vec(got, want.len());
        assert_eq!(
            got_i64.as_slice(),
            *want,
            "binding {} mismatch:\n  got     = {:?}\n  expected = {:?}\n--- WGSL ---\n{}",
            i,
            got_i64,
            want,
            wgsl
        );
    }
}

struct DeviceContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// The WGSL backend now emits `array<i64>` for Miri's default `int`, so
/// compute-test dispatch needs `Features::SHADER_INT64` on the device.
/// When the adapter does not expose it (older Metal, software fallbacks),
/// we return `None` and the caller skips, leaving the suite green.
fn try_init_wgpu() -> Option<DeviceContext> {
    let instance = Instance::new(InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    if !adapter.features().contains(Features::SHADER_INT64) {
        return None;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(
        &DeviceDescriptor {
            label: Some("miri gpu_wgsl test device"),
            required_features: Features::SHADER_INT64,
            required_limits: Limits::default(),
        },
        None,
    ))
    .ok()?;
    Some(DeviceContext { device, queue })
}

fn dispatch_compute(
    ctx: &DeviceContext,
    wgsl: &str,
    entry_point: &str,
    inputs: &[&[u8]],
) -> Vec<Vec<u8>> {
    let module = ctx.device.create_shader_module(ShaderModuleDescriptor {
        label: Some("gpu_wgsl_test_shader"),
        source: ShaderSource::Wgsl(wgsl.into()),
    });
    let storage_buffers = build_storage_buffers(ctx, inputs);
    let bind_group_layout = build_bind_group_layout(ctx, inputs.len());
    let pipeline = build_pipeline(ctx, &module, &bind_group_layout, entry_point);
    let bind_group = build_bind_group(ctx, &bind_group_layout, &storage_buffers);
    let grid_x = compute_grid_x(wgsl, inputs);

    let mut encoder = ctx
        .device
        .create_command_encoder(&CommandEncoderDescriptor {
            label: Some("gpu_wgsl_test_encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("gpu_wgsl_test_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(grid_x, 1, 1);
    }
    ctx.queue.submit(std::iter::once(encoder.finish()));
    ctx.device.poll(Maintain::Wait);

    storage_buffers
        .iter()
        .zip(inputs.iter())
        .map(|(buf, src)| readback(ctx, buf, src.len()))
        .collect()
}

/// Pick the workgroup count along x so every element of the largest input
/// is covered by at least one thread. Assumes 8-byte storage elements
/// (matches the WGSL `i64`/`f64` scalars used by the current helpers) and
/// that the kernel's iteration range does not exceed any binding's
/// length. Falls back to 1 group when the WGSL lacks a parseable
/// `@workgroup_size` attribute.
fn compute_grid_x(wgsl: &str, inputs: &[&[u8]]) -> u32 {
    let block_x = parse_block_x(wgsl).unwrap_or(1);
    let max_elems = inputs.iter().map(|b| b.len() / 8).max().unwrap_or(1).max(1) as u32;
    max_elems.div_ceil(block_x)
}

fn parse_block_x(wgsl: &str) -> Option<u32> {
    let after = wgsl.split("@workgroup_size(").nth(1)?;
    let first_arg = after.split(',').next()?.trim();
    first_arg.parse().ok()
}

fn build_storage_buffers(ctx: &DeviceContext, inputs: &[&[u8]]) -> Vec<wgpu::Buffer> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, data)| {
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("gpu_wgsl_test_storage_{}", i)),
                    contents: data,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                })
        })
        .collect()
}

fn build_bind_group_layout(ctx: &DeviceContext, num_bindings: usize) -> wgpu::BindGroupLayout {
    let entries: Vec<BindGroupLayoutEntry> = (0..num_bindings)
        .map(|i| BindGroupLayoutEntry {
            binding: i as u32,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    ctx.device
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("gpu_wgsl_test_bgl"),
            entries: &entries,
        })
}

fn build_pipeline(
    ctx: &DeviceContext,
    module: &wgpu::ShaderModule,
    bind_group_layout: &wgpu::BindGroupLayout,
    entry_point: &str,
) -> wgpu::ComputePipeline {
    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("gpu_wgsl_test_pl"),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });
    ctx.device
        .create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("gpu_wgsl_test_pipeline"),
            layout: Some(&pipeline_layout),
            module,
            entry_point,
            compilation_options: Default::default(),
        })
}

fn build_bind_group(
    ctx: &DeviceContext,
    bind_group_layout: &wgpu::BindGroupLayout,
    storage_buffers: &[wgpu::Buffer],
) -> wgpu::BindGroup {
    let entries: Vec<BindGroupEntry> = storage_buffers
        .iter()
        .enumerate()
        .map(|(i, b)| BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    ctx.device.create_bind_group(&BindGroupDescriptor {
        label: Some("gpu_wgsl_test_bg"),
        layout: bind_group_layout,
        entries: &entries,
    })
}

fn readback(ctx: &DeviceContext, src: &wgpu::Buffer, byte_len: usize) -> Vec<u8> {
    let padded = align_to_4(byte_len.max(4));
    let staging = ctx.device.create_buffer(&BufferDescriptor {
        label: Some("gpu_wgsl_test_readback"),
        size: padded as u64,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
        .device
        .create_command_encoder(&CommandEncoderDescriptor {
            label: Some("gpu_wgsl_test_readback_encoder"),
        });
    encoder.copy_buffer_to_buffer(src, 0, &staging, 0, padded as u64);
    ctx.queue.submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    ctx.device.poll(Maintain::Wait);
    rx.recv()
        .expect("map_async channel closed")
        .expect("map_async failed");

    let mapped = slice.get_mapped_range();
    let mut out = vec![0u8; byte_len];
    out.copy_from_slice(&mapped[..byte_len]);
    drop(mapped);
    staging.unmap();
    out
}

fn align_to_4(value: usize) -> usize {
    (value + 3) & !3
}

/// WebGPU storage buffers are little-endian per spec; encode host i64s the
/// same way so the layout matches regardless of the host architecture.
fn i64_slice_to_bytes(values: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn bytes_to_i64_vec(bytes: &[u8], len: usize) -> Vec<i64> {
    assert!(
        bytes.len() >= len * 8,
        "readback shorter than expected element count"
    );
    (0..len)
        .map(|i| {
            let start = i * 8;
            i64::from_le_bytes([
                bytes[start],
                bytes[start + 1],
                bytes[start + 2],
                bytes[start + 3],
                bytes[start + 4],
                bytes[start + 5],
                bytes[start + 6],
                bytes[start + 7],
            ])
        })
        .collect()
}
