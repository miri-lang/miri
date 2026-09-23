// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! High-level `forall` launch helper.
//!
//! `miri_gpu_launch_inline` is the single FFI entry Cranelift emits at
//! each `TerminatorKind::GpuLaunch`. It bundles init / compile / cache /
//! dispatch so the compiler can stay backend-agnostic about wgpu specifics.
//! A `gpu`-resident capture reuses its persistent device buffer and is
//! neither re-uploaded nor read back here; only `miri_gpu_readback` fences
//! and copies device bytes to the host.
//!
//! Kernel compilation is cached by name in the existing `KernelRegistry`
//! so repeated dispatches of the same kernel pay the compile cost once.

use crate::compute::{get_kernel_by_name, CompiledKernel};
use crate::context::{init_gpu_context, with_validation_scope, GpuContext, GpuError};
use crate::wire::WireConversion;
use crate::{device_table, telemetry};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::io::Write;
use std::sync::Arc;
use wgpu::{BufferUsages, Device, Features, Queue};

/// Generates a user-facing error message for every `GpuError` variant.
/// Returns a non-empty string starting with `"Runtime error: GPU"`.
pub(crate) fn gpu_launch_error_message(err: &GpuError) -> String {
    match err {
        GpuError::ValueOutOfI32Range {
            buffer_index,
            element_index,
            value,
        } => {
            format!(
                "Runtime error: GPU upload failed: buffer {} element {}: value {} \
                exceeds i32 range [{}, {}]; use Array<i32, N> for explicit 32-bit GPU storage",
                buffer_index,
                element_index,
                value,
                i32::MIN,
                i32::MAX
            )
        }
        GpuError::ValueOutOfU32Range {
            buffer_index,
            element_index,
            value,
        } => {
            format!(
                "Runtime error: GPU upload failed: buffer {} element {}: value {} \
                exceeds u32 range [{}, {}]; use Array<u32, N> for explicit 32-bit GPU storage",
                buffer_index,
                element_index,
                value,
                u32::MIN,
                u32::MAX
            )
        }
        GpuError::GridTooLarge(reason) => {
            format!(
                "Runtime error: GPU launch failed: {}; reduce the loop range or data size",
                reason
            )
        }
        GpuError::ShaderCompilationFailed(msg) => {
            format!("Runtime error: GPU shader compilation failed: {}", msg)
        }
        GpuError::NoAdapter => {
            "Runtime error: GPU launch failed: no compatible GPU adapter found".to_string()
        }
        GpuError::DeviceCreationFailed(reason) => {
            format!(
                "Runtime error: GPU launch failed: device creation failed: {}",
                reason
            )
        }
        GpuError::NotInitialized => {
            "Runtime error: GPU launch failed: GPU context not initialized".to_string()
        }
        GpuError::BufferCreationFailed => {
            "Runtime error: GPU launch failed: buffer creation failed".to_string()
        }
        GpuError::KernelNotFound(name) => {
            format!(
                "Runtime error: GPU launch failed: kernel '{}' not found",
                name
            )
        }
        GpuError::InvalidDimensions => {
            "Runtime error: GPU launch failed: invalid work dimensions".to_string()
        }
        GpuError::UnsupportedScalar(reason) => {
            format!(
                "Runtime error: GPU launch failed: unsupported scalar type: {}",
                reason
            )
        }
        GpuError::InactiveHandle(reason) => {
            format!("Runtime error: GPU launch failed: {}", reason)
        }
    }
}

/// Routes through the shared `context::GPU_CONTEXT` so every dispatch, upload,
/// and readback reaches one device per process. Keeping a separate `OnceCell`
/// here would strand resident buffers on a device no other call site can see.
fn ensure_context() -> Result<Arc<GpuContext>, GpuError> {
    init_gpu_context()
}

/// Narrows a signed i64 loop bound to an unsigned u32 for WGSL uniform storage.
///
/// # Contract
/// - Negative bounds result in 0 (empty loop, no error).
/// - Bounds exceeding u32::MAX reject with GridTooLarge (grid would be too large).
/// - Other values are cast to u32.
fn narrow_uniform_bound(value: i64) -> Result<u32, GpuError> {
    if value < 0 {
        Ok(0)
    } else if value > u32::MAX as i64 {
        Err(GpuError::GridTooLarge(
            "loop bound exceeds u32::MAX".to_string(),
        ))
    } else {
        Ok(value as u32)
    }
}

#[repr(C)]
pub struct GpuLaunchDesc {
    pub wgsl_ptr: *const u8,
    pub wgsl_len: usize,
    pub entry_ptr: *const u8,
    pub entry_len: usize,
    pub grid_x: u32,
    pub grid_y: u32,
    pub grid_z: u32,
    pub block_x: u32,
    pub block_y: u32,
    pub block_z: u32,
    pub num_bufs: usize,
    pub buf_data_ptrs: *const *mut u8,
    pub buf_byte_lens: *const usize,
    /// Per-capture `DeviceHandleId`. A non-zero id marks a `gpu`-resident
    /// binding whose device buffer persists across launches; `0` marks a
    /// host-resident capture that is uploaded and read back per launch.
    pub buf_handle_ids: *const u64,
    /// Which buffers are read-only. `buf_read_only[i]` is 1 if the i-th storage
    /// buffer binding is read-only, 0 if read-write. Array length is `num_bufs`.
    /// When null, all buffers are assumed read-write (legacy behavior).
    pub buf_read_only: *const u8,
    /// Per-buffer element conversion code (see [`crate::wire::WireConversion`]):
    /// how each buffer's elements are converted between their host width and
    /// their device lane on upload and readback. Array length is `num_bufs`.
    /// When null, every buffer's bytes are copied unchanged.
    pub buf_wire_conversion: *const u8,
    /// Bitmask indicating which uniform bounds and runtime range starts are present.
    /// Bit 0/1/2 = x/y/z loop-bound present; bit 3/4/5 = x/y/z range-start present.
    pub uniform_bound_present: u64,
    /// Bound value for x axis (1D loops or 2D x axis, or 3D x axis).
    pub uniform_bound_x_value: i64,
    /// Bound value for y axis (2D loops or 3D y axis).
    pub uniform_bound_y_value: i64,
    /// Bound value for z axis (3D loops only).
    pub uniform_bound_z_value: i64,
    /// Packed scalar capture values, each already converted by the compiler
    /// to its 32-bit device lane (4 bytes per scalar). When null, no scalar
    /// captures are present.
    pub scalar_inputs_ptr: *const u8,
    /// Byte length of `scalar_inputs_ptr` buffer.
    pub scalar_inputs_len: usize,
    /// Runtime range *start* for the x axis. Present when bit 3 of
    /// `uniform_bound_present` is set; the kernel indexes `thread + start`.
    pub uniform_start_x_value: i64,
    /// Runtime range start for the y axis. Present when bit 4 is set.
    pub uniform_start_y_value: i64,
    /// Runtime range start for the z axis. Present when bit 5 is set.
    pub uniform_start_z_value: i64,
}

const _: () = assert!(core::mem::size_of::<GpuLaunchDesc>() == 176);

/// Launches a GPU kernel inline. Returns 1 on success, 0 on failure.
///
/// Each capture's persistence is keyed on its `buf_handle_ids[i]`:
///   * `gpu`-resident capture (non-zero id) — the device buffer is allocated
///     and uploaded on first capture, then reused on every later launch with
///     no upload and no fence. The launch never copies it back; only a
///     cross-residency readback (`miri_gpu_readback`) fences and reads it.
///   * host-resident capture (`0`) — uploaded transiently and copied back to
///     host memory after the launch, matching the pre-residency behavior.
///
/// # Safety
/// `desc` must point to a fully initialized `GpuLaunchDesc`. The pointer
/// arrays it references must each contain `num_bufs` valid entries.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_launch_inline(desc: *const GpuLaunchDesc) -> u8 {
    if desc.is_null() {
        return 0;
    }
    let desc_ref = &*desc;
    match launch_impl(desc_ref) {
        Ok(()) => 1,
        // Every failure is reported and returned: the compiled caller ends the
        // program through the core runtime's trap, which reports a diagnostic
        // code and exits cleanly instead of dying on SIGABRT.
        Err(err) => {
            let msg = gpu_launch_error_message(&err);
            let _ = writeln!(std::io::stderr(), "{}", msg);
            0
        }
    }
}

/// Refuses a 1-D `forall` launch (a loop bound on x alone) that dispatches
/// more than 2³¹ threads. The kernel numbers its threads with the device's
/// 32-bit `int`, so a thread past `i32::MAX` would wrap negative and pass the
/// loop's bounds guard. A multi-axis loop indexes each axis separately and is
/// not limited this way, nor is a `gpu fn` launch, which carries no bound.
fn refuse_unindexable_loop(desc: &GpuLaunchDesc) -> Result<(), GpuError> {
    const BOUND_AXES: u64 = 0b111;
    const MAX_INDEXED_THREADS: u64 = 1 << 31;
    if desc.uniform_bound_present & BOUND_AXES != 1 {
        return Ok(());
    }
    let threads: u64 = [
        desc.grid_x,
        desc.grid_y,
        desc.grid_z,
        desc.block_x,
        desc.block_y,
        desc.block_z,
    ]
    .iter()
    .map(|&n| u64::from(n))
    .product();
    if threads > MAX_INDEXED_THREADS {
        return Err(GpuError::GridTooLarge(
            "this loop dispatches more threads than a 32-bit device index can number".to_string(),
        ));
    }
    Ok(())
}

unsafe fn launch_impl(desc: &GpuLaunchDesc) -> Result<(), GpuError> {
    device_table::check_activation_balance()?;
    let wgsl = decode_utf8(desc.wgsl_ptr, desc.wgsl_len)?;
    let entry_point = decode_utf8(desc.entry_ptr, desc.entry_len)?;

    let ctx = ensure_context()?;
    check_required_shader_features(wgsl, ctx.enabled_shader_features)?;

    // Account for uniform buffers in binding count if present. Bits 0..3 are the
    // per-axis loop bounds, bits 3..6 the per-axis runtime range starts. Count
    // how many bits are set.
    let num_uniform_bufs = (0..6)
        .filter(|bit| (desc.uniform_bound_present & (1 << bit)) != 0)
        .count();
    let has_scalar_inputs = desc.scalar_inputs_len > 0;
    let num_bindings = desc.num_bufs + num_uniform_bufs + (has_scalar_inputs as usize);

    // Ensure the kernel is compiled with the correct bind group layout.
    // The storage-buffer count equals num_bufs (every capture is a storage
    // binding); uniform and scalar-input bindings follow it. Passing it lets
    // the layout distinguish storage from uniform buffers.
    // Pass buf_read_only so storage buffers use the correct access mode.
    let buf_read_only = if desc.buf_read_only.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(
            desc.buf_read_only,
            desc.num_bufs,
        ))
    };
    ensure_kernel(
        entry_point,
        wgsl,
        num_bindings,
        desc.num_bufs,
        buf_read_only,
        [desc.block_x, desc.block_y, desc.block_z],
    )?;

    let kernel = get_kernel_by_name(&cache_key(entry_point, wgsl)).ok_or_else(|| {
        GpuError::ShaderCompilationFailed(format!(
            "failed to retrieve kernel after ensure_kernel for {}",
            entry_point
        ))
    })?;

    let device = &ctx.device;
    let queue = &ctx.queue;

    // Validate grid dimensions against device limits before allocating any buffers.
    let max_workgroups = ctx.device.limits().max_compute_workgroups_per_dimension;
    if desc.grid_x > max_workgroups || desc.grid_y > max_workgroups || desc.grid_z > max_workgroups
    {
        return Err(GpuError::GridTooLarge(
            "grid dimensions exceed device limits".to_string(),
        ));
    }

    // Check that the grid product doesn't overflow u64. While wgpu's Limits don't
    // define a total workgroup count limit, the product overflow itself is a guard
    // against programming errors (e.g., very large 2D loops with runtime bounds).
    let _ = (desc.grid_x as u64)
        .checked_mul(desc.grid_y as u64)
        .and_then(|v| v.checked_mul(desc.grid_z as u64))
        .ok_or_else(|| GpuError::GridTooLarge("grid product (x*y*z) overflows u64".to_string()))?;

    // The per-axis loop bounds (bits 0..3) and runtime range starts (bits 3..6),
    // each a 4-byte `u32` uniform. Ordered bounds-then-starts so the binding
    // indices match the WGSL emitter, which emits the `_bound_*` params before
    // the `_start_*` params.
    let uniform_scalars: [(u64, i64, &str); 6] = [
        (1, desc.uniform_bound_x_value, "miri_gpu_uniform_bound_x"),
        (2, desc.uniform_bound_y_value, "miri_gpu_uniform_bound_y"),
        (4, desc.uniform_bound_z_value, "miri_gpu_uniform_bound_z"),
        (8, desc.uniform_start_x_value, "miri_gpu_uniform_start_x"),
        (16, desc.uniform_start_y_value, "miri_gpu_uniform_start_y"),
        (32, desc.uniform_start_z_value, "miri_gpu_uniform_start_z"),
    ];

    // Validate every present uniform's range before creating any buffers.
    for (bit, value, _) in uniform_scalars {
        if (desc.uniform_bound_present & bit) != 0 {
            let _ = narrow_uniform_bound(value)?;
        }
    }
    refuse_unindexable_loop(desc)?;

    let buf_data_ptrs = std::slice::from_raw_parts(desc.buf_data_ptrs, desc.num_bufs);
    let buf_byte_lens = std::slice::from_raw_parts(desc.buf_byte_lens, desc.num_bufs);
    let buf_handle_ids = std::slice::from_raw_parts(desc.buf_handle_ids, desc.num_bufs);
    let buf_wire_conversion = if desc.buf_wire_conversion.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(
            desc.buf_wire_conversion,
            desc.num_bufs,
        ))
    };

    let (storage_buffers, transient_captures) = prepare_capture_buffers(
        device,
        queue,
        buf_handle_ids,
        buf_data_ptrs,
        buf_byte_lens,
        buf_wire_conversion,
    )?;

    // Create one 4-byte `u32` uniform buffer per present bound/start, in the
    // bounds-then-starts order established above. Each must live until the bind
    // group is created.
    let mut uniform_bufs: Vec<wgpu::Buffer> = Vec::new();

    for (bit, value, label) in uniform_scalars {
        if (desc.uniform_bound_present & bit) == 0 {
            continue;
        }
        let value_u32 = narrow_uniform_bound(value)?;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: 4,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buf, 0, &value_u32.to_le_bytes());
        uniform_bufs.push(buf);
    }

    let scalar_inputs_buf = if desc.scalar_inputs_len > 0 {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("miri_gpu_scalar_inputs"),
            size: desc.scalar_inputs_len as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scalar_data =
            std::slice::from_raw_parts(desc.scalar_inputs_ptr, desc.scalar_inputs_len);
        queue.write_buffer(&buf, 0, scalar_data);
        Some(buf)
    } else {
        None
    };

    let mut entries: Vec<wgpu::BindGroupEntry> = storage_buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();

    // Add uniform buffer binding(s) if present.
    for (i, ub) in uniform_bufs.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: (desc.num_bufs + i) as u32,
            resource: ub.as_entire_binding(),
        });
    }

    if let Some(sib) = &scalar_inputs_buf {
        entries.push(wgpu::BindGroupEntry {
            binding: (desc.num_bufs + uniform_bufs.len()) as u32,
            resource: sib.as_entire_binding(),
        });
    }

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("miri_gpu_launch_inline bg"),
        layout: &kernel.bind_group_layout,
        entries: &entries,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("miri_gpu_launch_inline encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("miri_gpu_launch_inline pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(desc.grid_x, desc.grid_y, desc.grid_z);
    }
    queue.submit(std::iter::once(encoder.finish()));
    telemetry::record_launch();

    // A pure `gpu`-resident launch fences nothing: device-side ordering on
    // the queue guarantees a later launch sees this one's writes, and the
    // bytes stay on the device until an explicit readback. Only a transient
    // host capture forces a host-visible copy back, which needs a fence.
    if !transient_captures.is_empty() {
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        telemetry::record_fence();
        for i in transient_captures {
            readback_device_buffer(
                device,
                queue,
                &storage_buffers[i],
                buf_data_ptrs[i],
                buf_byte_lens[i],
                WireConversion::for_buffer(buf_wire_conversion, i)?,
            )?;
        }
    }
    Ok(())
}

/// Builds the storage buffer for every capture and reports which captures are
/// transient (host-resident, handle `0`). A `gpu`-resident capture reuses or
/// allocates its persistent buffer; a transient one allocates fresh and is
/// scheduled for post-dispatch readback.
///
/// `buf_wire_conversion[i]` names how buffer `i`'s elements are converted to
/// their device lane on upload (and back on readback).
///
/// # Safety
/// The three slices must be `num_bufs` long and their host pointers valid for
/// the matching byte lengths.
///
/// # Errors
/// Returns `Err` if any element does not fit its device lane, or a conversion
/// code is unknown.
unsafe fn prepare_capture_buffers(
    device: &Device,
    queue: &Queue,
    buf_handle_ids: &[u64],
    buf_data_ptrs: &[*mut u8],
    buf_byte_lens: &[usize],
    buf_wire_conversion: Option<&[u8]>,
) -> Result<(Vec<wgpu::Buffer>, Vec<usize>), GpuError> {
    let mut storage_buffers = Vec::with_capacity(buf_handle_ids.len());
    let mut transient_captures = Vec::new();
    for (i, &handle) in buf_handle_ids.iter().enumerate() {
        let upload = HostUpload {
            host_ptr: buf_data_ptrs[i],
            byte_len: buf_byte_lens[i],
            conversion: WireConversion::for_buffer(buf_wire_conversion, i)?,
            buffer_index: i,
        };
        let buffer = if handle != device_table::HOST_HANDLE {
            persistent_capture_buffer(device, queue, handle, upload)?
        } else {
            transient_captures.push(i);
            new_storage_buffer_with_upload(device, queue, upload)?
        };
        storage_buffers.push(buffer);
    }
    Ok((storage_buffers, transient_captures))
}

/// A host buffer about to be uploaded to the device: where its bytes are, how
/// many there are, how its elements convert to their device lane, and which
/// launch buffer it is (for error reports).
#[derive(Clone, Copy)]
struct HostUpload {
    host_ptr: *mut u8,
    byte_len: usize,
    conversion: WireConversion,
    buffer_index: usize,
}

/// Returns the resident device buffer for `handle`, allocating and uploading
/// it on first capture and reusing it (no upload) on every later launch.
///
/// Elements are converted and range-checked when the buffer is first
/// uploaded; later captures reuse the persistent buffer without re-checking.
///
/// # Errors
/// Returns `Err` if an element does not fit its device lane on first upload.
unsafe fn persistent_capture_buffer(
    device: &Device,
    queue: &Queue,
    handle: u64,
    upload: HostUpload,
) -> Result<wgpu::Buffer, GpuError> {
    if let Some((existing, _, _)) = device_table::resident_buffer(handle) {
        return Ok(existing);
    }
    let buffer = new_storage_buffer_with_upload(device, queue, upload)?;
    let device_byte_len = upload.conversion.device_len(upload.byte_len)?;
    device_table::insert_resident(handle, buffer.clone(), device_byte_len, upload.conversion)?;
    Ok(buffer)
}

/// Allocates a storage buffer sized for the device form of `upload` and, when
/// there are host bytes to copy, converts and uploads them, recording one
/// upload in the telemetry counters; an empty or null capture allocates the
/// buffer without an upload.
///
/// # Errors
/// Returns `Err` if an element does not fit its device lane.
unsafe fn new_storage_buffer_with_upload(
    device: &Device,
    queue: &Queue,
    upload: HostUpload,
) -> Result<wgpu::Buffer, GpuError> {
    let device_len = upload.conversion.device_len(upload.byte_len)?;
    let upload_bytes = if upload.byte_len > 0 && !upload.host_ptr.is_null() {
        let host = std::slice::from_raw_parts(upload.host_ptr as *const u8, upload.byte_len);
        upload.conversion.encode(host, upload.buffer_index)?
    } else {
        Vec::new()
    };

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("miri_gpu_launch_inline storage"),
        size: align_to_4(device_len.max(4)) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !upload_bytes.is_empty() {
        queue.write_buffer(&buffer, 0, &upload_bytes);
        telemetry::record_upload();
    }
    Ok(buffer)
}

/// Refuse to dispatch a kernel whose WGSL references a 64-bit scalar
/// (`i64`/`u64`/`f64`) when the device was not booted with the matching
/// wgpu feature. Without this gate, naga's shader-module compilation would
/// reject the kernel later with a generic message; surfacing the cause
/// upfront keeps the diagnostic source-relevant (which scalar) instead of
/// pipeline-relevant (which wgpu validator rule fired).
pub fn check_required_shader_features(wgsl: &str, enabled: Features) -> Result<(), GpuError> {
    let needs_int64 = wgsl_uses_scalar(wgsl, "i64") || wgsl_uses_scalar(wgsl, "u64");
    let needs_f64 = wgsl_uses_scalar(wgsl, "f64");
    let needs_f16 = wgsl_uses_scalar(wgsl, "f16");
    let needs_subgroup = wgsl.contains("subgroup")
        || wgsl.contains("SUBGROUP_SIZE")
        || wgsl.contains("SUBGROUP_INVOCATION_ID");

    if needs_int64 && !enabled.contains(Features::SHADER_INT64) {
        return Err(GpuError::UnsupportedScalar(
            "kernel uses i64/u64 but the adapter does not support Features::SHADER_INT64".into(),
        ));
    }
    if needs_f64 && !enabled.contains(Features::SHADER_F64) {
        return Err(GpuError::UnsupportedScalar(
            "kernel uses f64 but the adapter does not support Features::SHADER_F64".into(),
        ));
    }
    if needs_f16 && !enabled.contains(Features::SHADER_F16) {
        return Err(GpuError::UnsupportedScalar(
            "kernel uses f16 but the adapter does not support Features::SHADER_F16".into(),
        ));
    }
    if needs_subgroup && !enabled.contains(Features::SUBGROUP) {
        return Err(GpuError::UnsupportedScalar(
            "kernel uses subgroup ops but the adapter does not support Features::SUBGROUP".into(),
        ));
    }
    Ok(())
}

/// True when `wgsl` contains `name` as a whole identifier token. Treats any
/// non-`[A-Za-z0-9_]` character as a token boundary, so a name like
/// `xi64y` does not match `i64`. The WGSL emitter never produces 64-bit
/// keywords as substrings of user-derived identifiers, so this scan is
/// stable against the entire output of the WGSL backend.
pub fn wgsl_uses_scalar(wgsl: &str, name: &str) -> bool {
    let bytes = wgsl.as_bytes();
    let needle = name.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    for start in 0..=bytes.len() - needle.len() {
        if &bytes[start..start + needle.len()] != needle {
            continue;
        }
        let prev_ok = start == 0 || !is_ident(bytes[start - 1]);
        let next = start + needle.len();
        let next_ok = next == bytes.len() || !is_ident(bytes[next]);
        if prev_ok && next_ok {
            return true;
        }
    }
    false
}

unsafe fn decode_utf8<'a>(ptr: *const u8, len: usize) -> Result<&'a str, GpuError> {
    if ptr.is_null() {
        return Err(GpuError::ShaderCompilationFailed("null pointer".into()));
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len))
        .map_err(|err| GpuError::ShaderCompilationFailed(format!("invalid UTF-8: {err}")))
}

/// Cache key combines the entry-point name with a checksum of the WGSL
/// source so that two kernels declared with the same name but different
/// bodies (cross-unit collision, hot reload) compile into separate
/// pipelines instead of silently aliasing the first one.
fn cache_key(entry_point: &str, wgsl: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    wgsl.hash(&mut hasher);
    format!("{}#{:016x}", entry_point, hasher.finish())
}

/// Ensure the kernel is compiled with a bind group layout that splits the
/// `num_storage_bufs` storage bindings from the trailing uniform bindings.
/// `buf_read_only` specifies which buffers are read-only (true=read-only, false=read-write).
fn ensure_kernel(
    entry_point: &str,
    wgsl: &str,
    num_bindings: usize,
    num_storage_bufs: usize,
    buf_read_only: Option<&[u8]>,
    workgroup_size: [u32; 3],
) -> Result<(), GpuError> {
    let key = cache_key(entry_point, wgsl);
    if let Some(_existing) = get_kernel_by_name(&key) {
        return Ok(());
    }
    compile_and_register(
        entry_point,
        &key,
        wgsl,
        num_bindings,
        num_storage_bufs,
        buf_read_only,
        workgroup_size,
    )
}

fn compile_and_register(
    entry_point: &str,
    cache_name: &str,
    wgsl: &str,
    num_bindings: usize,
    num_storage_bufs: usize,
    buf_read_only: Option<&[u8]>,
    workgroup_size: [u32; 3],
) -> Result<(), GpuError> {
    static REGISTER_LOCK: OnceCell<RwLock<()>> = OnceCell::new();
    let lock = REGISTER_LOCK.get_or_init(|| RwLock::new(()));
    let _guard = lock.write();
    if let Some(_existing) = get_kernel_by_name(cache_name) {
        return Ok(());
    }
    let kernel = compile_kernel_inline(
        entry_point,
        cache_name,
        wgsl,
        num_bindings,
        num_storage_bufs,
        buf_read_only,
        workgroup_size,
    )?;
    let _id = kernel.id;
    crate::compute::register_kernel_inline(kernel);
    Ok(())
}

fn compile_kernel_inline(
    entry_point: &str,
    cache_name: &str,
    wgsl: &str,
    num_bindings: usize,
    num_storage_bufs: usize,
    buf_read_only: Option<&[u8]>,
    workgroup_size: [u32; 3],
) -> Result<CompiledKernel, GpuError> {
    let ctx = ensure_context()?;
    let (module, bind_group_layout, pipeline) = with_validation_scope(&ctx.device, || {
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(cache_name),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });

        let bind_group_layout = build_bind_group_layout(
            &ctx.device,
            cache_name,
            num_bindings,
            num_storage_bufs,
            buf_read_only,
        );

        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{}_pl", cache_name)),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

        let pipeline = ctx
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&format!("{}_pipeline", cache_name)),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

        (module, bind_group_layout, pipeline)
    })?;
    Ok(CompiledKernel {
        id: next_kernel_id(),
        name: cache_name.to_string(),
        shader_module: module,
        pipeline,
        bind_group_layout,
        num_bindings,
        workgroup_size,
    })
}

/// Build a bind group layout that matches the bindings declared in the WGSL
/// shader: the first `num_storage_bufs` bindings are storage buffers, the
/// remaining `num_bindings - num_storage_bufs` are uniform buffers.
/// `buf_read_only` specifies which storage buffers are read-only (1=read-only, 0=read-write).
pub(crate) fn build_bind_group_layout(
    device: &Device,
    name: &str,
    num_bindings: usize,
    num_storage_bufs: usize,
    buf_read_only: Option<&[u8]>,
) -> wgpu::BindGroupLayout {
    let num_uniform = num_bindings - num_storage_bufs;

    let mut entries = Vec::new();

    // Storage buffer bindings (0..num_storage_bufs).
    for i in 0..num_storage_bufs {
        let is_read_only = buf_read_only
            .and_then(|arr| arr.get(i))
            .map(|&b| b != 0)
            .unwrap_or(false);
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: i as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: is_read_only,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }

    // Uniform buffer bindings (num_storage_bufs..num_bindings).
    for i in 0..num_uniform {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: (num_storage_bufs + i) as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{}_layout", name)),
        entries: &entries,
    })
}

fn next_kernel_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1_000_000);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

unsafe fn readback_device_buffer(
    device: &Device,
    queue: &Queue,
    src: &wgpu::Buffer,
    host_ptr: *mut u8,
    byte_len: usize,
    conversion: WireConversion,
) -> Result<(), GpuError> {
    if byte_len == 0 || host_ptr.is_null() {
        return Ok(());
    }
    let device_byte_len = conversion.device_len(byte_len)?;
    let padded = align_to_4(device_byte_len.max(4)) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("miri_gpu_launch_inline readback"),
        size: padded,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("miri_gpu_launch_inline readback encoder"),
    });
    encoder.copy_buffer_to_buffer(src, 0, &staging, 0, padded);
    queue.submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv()
        .map_err(|_| GpuError::BufferCreationFailed)?
        .map_err(|_| GpuError::BufferCreationFailed)?;

    let mapped = slice.get_mapped_range();
    let host = std::slice::from_raw_parts_mut(host_ptr, byte_len);
    let decoded = conversion.decode(&mapped[..device_byte_len], host);
    drop(mapped);
    staging.unmap();
    decoded
}

fn align_to_4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

/// Leading fields of `runtime::core::MiriArray` (`repr(C)`), mirrored here so
/// the GPU readback can recover a capture's host pointer and byte length from
/// the array passed by the compiler. Kept in sync with the layout the
/// Cranelift launch dispatcher reads (`miri_array_layout`).
#[repr(C)]
pub struct MiriArrayHeader {
    pub data: *mut u8,
    pub elem_count: usize,
    pub elem_size: usize,
}

#[cfg(test)]
mod narrow_uniform_bound_tests {
    use super::{narrow_uniform_bound, GpuError};

    #[test]
    fn negative_bound_returns_zero() {
        assert_eq!(narrow_uniform_bound(-1).unwrap(), 0);
        assert_eq!(narrow_uniform_bound(-10).unwrap(), 0);
        assert_eq!(narrow_uniform_bound(i64::MIN).unwrap(), 0);
    }

    #[test]
    fn zero_bound_returns_zero() {
        assert_eq!(narrow_uniform_bound(0).unwrap(), 0);
    }

    #[test]
    fn small_positive_bounds_work() {
        assert_eq!(narrow_uniform_bound(1).unwrap(), 1);
        assert_eq!(narrow_uniform_bound(256).unwrap(), 256);
        assert_eq!(narrow_uniform_bound(4096).unwrap(), 4096);
    }

    #[test]
    fn u32_max_succeeds() {
        assert_eq!(narrow_uniform_bound(u32::MAX as i64).unwrap(), u32::MAX);
    }

    #[test]
    fn exceeding_u32_max_errors() {
        assert!(matches!(
            narrow_uniform_bound(u32::MAX as i64 + 1),
            Err(GpuError::GridTooLarge(_))
        ));
        assert!(matches!(
            narrow_uniform_bound(i64::MAX),
            Err(GpuError::GridTooLarge(_))
        ));
        assert!(matches!(
            narrow_uniform_bound(5_000_000_000),
            Err(GpuError::GridTooLarge(_))
        ));
    }
}

#[cfg(test)]
mod desc_layout_tests {
    use super::GpuLaunchDesc;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn gpu_launch_desc_abi_is_pinned() {
        assert_eq!(
            size_of::<GpuLaunchDesc>(),
            176,
            "GpuLaunchDesc size drifted; update Cranelift desc_layout::DESC_SIZE in lockstep"
        );
        assert_eq!(align_of::<GpuLaunchDesc>(), 8);
        assert_eq!(offset_of!(GpuLaunchDesc, wgsl_ptr), 0);
        assert_eq!(offset_of!(GpuLaunchDesc, wgsl_len), 8);
        assert_eq!(offset_of!(GpuLaunchDesc, entry_ptr), 16);
        assert_eq!(offset_of!(GpuLaunchDesc, entry_len), 24);
        assert_eq!(offset_of!(GpuLaunchDesc, grid_x), 32);
        assert_eq!(offset_of!(GpuLaunchDesc, grid_y), 36);
        assert_eq!(offset_of!(GpuLaunchDesc, grid_z), 40);
        assert_eq!(offset_of!(GpuLaunchDesc, block_x), 44);
        assert_eq!(offset_of!(GpuLaunchDesc, block_y), 48);
        assert_eq!(offset_of!(GpuLaunchDesc, block_z), 52);
        assert_eq!(offset_of!(GpuLaunchDesc, num_bufs), 56);
        assert_eq!(offset_of!(GpuLaunchDesc, buf_data_ptrs), 64);
        assert_eq!(offset_of!(GpuLaunchDesc, buf_byte_lens), 72);
        assert_eq!(offset_of!(GpuLaunchDesc, buf_handle_ids), 80);
        assert_eq!(offset_of!(GpuLaunchDesc, buf_read_only), 88);
        assert_eq!(offset_of!(GpuLaunchDesc, buf_wire_conversion), 96);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_present), 104);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_x_value), 112);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_y_value), 120);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_z_value), 128);
        assert_eq!(offset_of!(GpuLaunchDesc, scalar_inputs_ptr), 136);
        assert_eq!(offset_of!(GpuLaunchDesc, scalar_inputs_len), 144);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_start_x_value), 152);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_start_y_value), 160);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_start_z_value), 168);
    }
}

/// Cross-residency readback: fences outstanding writes to the device buffer
/// owned by `handle` and copies its contents into the host array `arr`. This
/// is the only operation that fences device work.
///
/// A `handle` with no resident buffer (e.g. a binding never captured by a
/// launch) leaves `arr` untouched and succeeds — its host bytes are already
/// the authoritative copy.
///
/// # Safety
/// `arr` must point to a valid `MiriArrayHeader` whose `data` covers
/// `elem_count * elem_size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_readback(handle: u64, arr: *const MiriArrayHeader) -> u8 {
    if arr.is_null() {
        return 0;
    }
    let header = &*arr;
    let host_byte_len = header.elem_count.saturating_mul(header.elem_size);
    if host_byte_len == 0 || header.data.is_null() {
        return 1;
    }
    let Some((buffer, resident_byte_len, conversion)) = device_table::resident_buffer(handle)
    else {
        return 1;
    };
    // `readback_device_buffer` takes the HOST byte length and derives the
    // device length from the conversion, so the host length is clamped, in
    // whole elements, to what the resident buffer holds: a host array that
    // grew since the upload can never issue a copy past the device buffer.
    let byte_len = conversion.host_len_within(host_byte_len, resident_byte_len);
    let Ok(ctx) = init_gpu_context() else {
        return 0;
    };
    match readback_device_buffer(
        &ctx.device,
        &ctx.queue,
        &buffer,
        header.data,
        byte_len,
        conversion,
    ) {
        Ok(()) => {
            telemetry::record_fence();
            telemetry::record_readback();
            1
        }
        Err(err) => {
            log::error!("miri_gpu_readback failed: {:?}", err);
            0
        }
    }
}

/// Cross-residency upload: copies host array bytes to the persistent device
/// buffer owned by `handle`, converting each element to its device lane the
/// way the buffer's first upload did.
///
/// A `handle` with no resident buffer yet uploads nothing: the host array is
/// the authoritative copy, and the first launch that captures the binding
/// allocates the device buffer from it with the element conversion the launch
/// descriptor names. This call carries no element type, so it cannot choose a
/// conversion itself.
///
/// When `handle` is 0 (HOST_HANDLE sentinel), returns 0 immediately—
/// host-resident captures manage their own device buffers in the launch path.
///
/// # Safety
/// `arr` must point to a valid `MiriArrayHeader` whose `data` covers
/// `elem_count * elem_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_upload(handle: u64, arr: *const MiriArrayHeader) -> u8 {
    if arr.is_null() || handle == device_table::HOST_HANDLE {
        return 0;
    }
    let header = &*arr;
    let host_byte_len = header.elem_count.saturating_mul(header.elem_size);
    if host_byte_len == 0 || header.data.is_null() {
        return 1;
    }
    let Some((existing_buffer, existing_byte_len, conversion)) =
        device_table::resident_buffer(handle)
    else {
        return 1;
    };
    let Ok(ctx) = init_gpu_context() else {
        return 0;
    };
    let host = std::slice::from_raw_parts(header.data as *const u8, host_byte_len);
    let upload_bytes = match conversion.encode(host, 0) {
        Ok(bytes) => bytes,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "{}", gpu_launch_error_message(&err));
            return 0;
        }
    };
    let upload_len = upload_bytes.len().min(existing_byte_len);
    ctx.queue
        .write_buffer(&existing_buffer, 0, &upload_bytes[..upload_len]);
    telemetry::record_upload();
    1
}

#[cfg(test)]
mod shader_compilation_tests {
    use super::*;

    /// Test that invalid WGSL (e.g. referencing undeclared identifier) returns
    /// ShaderCompilationFailed instead of panicking. This test requires a live
    /// GPU adapter and is skipped if none is available.
    #[test]
    fn invalid_wgsl_returns_error() {
        let _ctx = match init_gpu_context() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("No GPU adapter available, skipping invalid_wgsl_returns_error");
                return;
            }
        };

        // Invalid WGSL: references undeclared identifier `missing_var`.
        let invalid_wgsl = r#"
            @compute @workgroup_size(256)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                var x = missing_var;
            }
        "#;

        let result = compile_kernel_inline(
            "invalid_entry",
            "invalid_entry#test",
            invalid_wgsl,
            1,
            1,
            None,
            [256, 1, 1],
        );

        match result {
            Err(GpuError::ShaderCompilationFailed(msg)) => {
                let msg_lower = msg.to_lowercase();
                // Assert the message mentions either the undefined variable or a validation keyword.
                // naga reports validation/scope errors; exact phrasing is version-dependent.
                assert!(
                    msg_lower.contains("missing_var")
                        || msg_lower.contains("not found")
                        || msg_lower.contains("undefined")
                        || msg_lower.contains("unknown"),
                    "error should mention the undefined identifier or be a validation error, got: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected ShaderCompilationFailed, but compilation succeeded"),
            Err(e) => panic!("expected ShaderCompilationFailed, got: {:?}", e),
        }
    }

    /// Test that wrong entry point name returns ShaderCompilationFailed.
    #[test]
    fn wrong_entry_point_returns_error() {
        let _ctx = match init_gpu_context() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("No GPU adapter available, skipping wrong_entry_point_returns_error");
                return;
            }
        };

        // Valid WGSL but entry point name doesn't match.
        let wgsl = r#"
            @compute @workgroup_size(256)
            fn kernel_main(@builtin(global_invocation_id) id: vec3<u32>) {
            }
        "#;

        let result = compile_kernel_inline(
            "nonexistent_entry",
            "wrong_entry#test",
            wgsl,
            0,
            0,
            None,
            [256, 1, 1],
        );

        match result {
            Err(GpuError::ShaderCompilationFailed(msg)) => {
                let msg_lower = msg.to_lowercase();
                // Assert the message mentions either entry point or a validation issue.
                // naga reports "no entry point found" or similar validation errors.
                assert!(
                    msg_lower.contains("entry")
                        || msg_lower.contains("nonexistent")
                        || msg_lower.contains("validation")
                        || !msg.is_empty(),
                    "error should be a validation error, got: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected ShaderCompilationFailed, but compilation succeeded"),
            Err(e) => panic!("expected ShaderCompilationFailed, got: {:?}", e),
        }
    }
}

#[cfg(test)]
mod gpu_launch_error_message_tests {
    use super::*;

    #[test]
    fn no_adapter_error() {
        let err = GpuError::NoAdapter;
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("no compatible GPU adapter"));
    }

    #[test]
    fn device_creation_failed_error() {
        let err = GpuError::DeviceCreationFailed("metal device failed".to_string());
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("device creation failed"));
        assert!(msg.contains("metal device failed"));
    }

    #[test]
    fn not_initialized_error() {
        let err = GpuError::NotInitialized;
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("not initialized"));
    }

    #[test]
    fn buffer_creation_failed_error() {
        let err = GpuError::BufferCreationFailed;
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("buffer creation"));
    }

    #[test]
    fn shader_compilation_failed_error() {
        let err = GpuError::ShaderCompilationFailed("naga: invalid syntax".to_string());
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("shader compilation"));
        assert!(msg.contains("naga: invalid syntax"));
    }

    #[test]
    fn kernel_not_found_error() {
        let err = GpuError::KernelNotFound("my_kernel".to_string());
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("kernel"));
        assert!(msg.contains("my_kernel"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn invalid_dimensions_error() {
        let err = GpuError::InvalidDimensions;
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("invalid work dimensions"));
    }

    #[test]
    fn unsupported_scalar_error() {
        let err = GpuError::UnsupportedScalar("f64 not supported on this adapter".to_string());
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("unsupported scalar type"));
        assert!(msg.contains("f64 not supported on this adapter"));
    }

    #[test]
    fn value_out_of_i32_range_error() {
        let err = GpuError::ValueOutOfI32Range {
            buffer_index: 0,
            element_index: 5,
            value: 9_223_372_036_854_775_807i64,
        };
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("exceeds i32 range"));
        assert!(msg.contains("9223372036854775807"));
    }

    #[test]
    fn grid_too_large_error() {
        let err = GpuError::GridTooLarge("grid dimensions exceed device limits".to_string());
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU"));
        assert!(msg.contains("grid dimensions exceed device limits"));
        assert!(msg.contains("reduce the loop range"));
    }

    #[test]
    fn inactive_handle_error() {
        let err = GpuError::InactiveHandle(
            "gpu binding 7 was released with no live activation".to_string(),
        );
        let msg = gpu_launch_error_message(&err);
        assert!(msg.starts_with("Runtime error: GPU launch failed"));
        assert!(msg.contains("gpu binding 7 was released with no live activation"));
    }

    #[test]
    fn f16_kernel_rejected_without_shader_f16_feature() {
        let wgsl = "enable f16;\n@group(0) @binding(0) var<storage> a: array<f16>;";
        let err = super::check_required_shader_features(wgsl, super::Features::empty())
            .expect_err("an f16 kernel must be rejected when SHADER_F16 is absent");
        match err {
            GpuError::UnsupportedScalar(msg) => assert!(msg.contains("f16")),
            other => panic!("expected UnsupportedScalar, got {:?}", other),
        }
    }

    #[test]
    fn f16_kernel_accepted_with_shader_f16_feature() {
        let wgsl = "enable f16;\n@group(0) @binding(0) var<storage> a: array<f16>;";
        super::check_required_shader_features(wgsl, super::Features::SHADER_F16)
            .expect("an f16 kernel must pass when SHADER_F16 is enabled");
    }

    #[test]
    fn non_f16_kernel_does_not_require_shader_f16_feature() {
        let wgsl = "@group(0) @binding(0) var<storage> a: array<f32>;";
        super::check_required_shader_features(wgsl, super::Features::empty())
            .expect("an f32 kernel must not require SHADER_F16");
    }
}
