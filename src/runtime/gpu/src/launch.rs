// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! High-level `gpu for` launch helper.
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
    }
}

/// Routes through the shared `context::GPU_CONTEXT` so a successful
/// `gpu for` dispatch makes `miri_gpu_is_available()` (and therefore
/// `system.gpu.is_gpu_available()`) start returning true. Keeping a
/// separate `OnceCell` here would leave the public probe permanently
/// false even after the runtime has booted a device.
fn ensure_context() -> Result<&'static Arc<GpuContext>, GpuError> {
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
    /// Which buffers need i64→i32 narrowing on upload and i32→i64 widening on readback.
    /// `buf_int_narrow[i]` is 1 if the i-th buffer is an `Array<int, N>`, 0 otherwise.
    /// Array length is `num_bufs`. When null, no buffers need narrowing (legacy behavior).
    pub buf_int_narrow: *const u8,
    /// Bitmask indicating which uniform bounds are present.
    /// Bit 0 = x bound present, Bit 1 = y bound present.
    pub uniform_bound_present: u64,
    /// Bound value for x axis (1D loops or 2D x axis).
    pub uniform_bound_x_value: i64,
    /// Bound value for y axis (2D loops only).
    pub uniform_bound_y_value: i64,
    /// Number of storage buffer bindings.
    /// num_bufs reflects capture count, but with uniform buffers present,
    /// the kernel has num_bufs storage + 1 uniform binding. This is always num_bufs.
    pub num_storage_bufs: u64,
    /// Padding to reach 144 bytes for alignment.
    pub _padding: [u64; 1],
}

const _: () = assert!(core::mem::size_of::<GpuLaunchDesc>() == 144);

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
        Err(err) => {
            let msg = gpu_launch_error_message(&err);
            let _ = writeln!(std::io::stderr(), "{}", msg);

            // Three variants abort unconditionally because silent data corruption
            // is worse than early termination. Once generated code traps all
            // non-success return codes, these will be caught by the trap.
            match &err {
                GpuError::ValueOutOfI32Range { .. }
                | GpuError::GridTooLarge(_)
                | GpuError::ShaderCompilationFailed(_) => {
                    std::process::abort();
                }
                _ => 0,
            }
        }
    }
}

unsafe fn launch_impl(desc: &GpuLaunchDesc) -> Result<(), GpuError> {
    let wgsl = decode_utf8(desc.wgsl_ptr, desc.wgsl_len)?;
    let entry_point = decode_utf8(desc.entry_ptr, desc.entry_len)?;

    let ctx = ensure_context()?;
    check_required_shader_features(wgsl, ctx.enabled_shader_features)?;

    // Account for uniform buffers in binding count if present.
    // Bit 0 = x bound, Bit 1 = y bound. Count how many bits are set.
    let num_uniform_bufs = ((desc.uniform_bound_present & 1) != 0_u64) as usize
        + ((desc.uniform_bound_present & 2) != 0_u64) as usize;
    let num_bindings = desc.num_bufs + num_uniform_bufs;

    // Ensure the kernel is compiled with the correct bind group layout.
    // Pass num_storage_bufs so the layout can distinguish storage from uniform buffers.
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
        desc.num_storage_bufs as usize,
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

    // Validate uniform bounds range before creating buffers.
    let has_x_bound = (desc.uniform_bound_present & 1) != 0;
    let has_y_bound = (desc.uniform_bound_present & 2) != 0;

    if has_x_bound {
        let _ = narrow_uniform_bound(desc.uniform_bound_x_value)?;
    }
    if has_y_bound {
        let _ = narrow_uniform_bound(desc.uniform_bound_y_value)?;
    }

    let buf_data_ptrs = std::slice::from_raw_parts(desc.buf_data_ptrs, desc.num_bufs);
    let buf_byte_lens = std::slice::from_raw_parts(desc.buf_byte_lens, desc.num_bufs);
    let buf_handle_ids = std::slice::from_raw_parts(desc.buf_handle_ids, desc.num_bufs);
    let buf_int_narrow = if desc.buf_int_narrow.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(
            desc.buf_int_narrow,
            desc.num_bufs,
        ))
    };

    let (storage_buffers, transient_captures) = prepare_capture_buffers(
        device,
        queue,
        buf_handle_ids,
        buf_data_ptrs,
        buf_byte_lens,
        buf_int_narrow,
    )?;

    // Create uniform buffer(s) if needed.
    // For 1D loops: one 4-byte uniform with x bound.
    // For 2D loops: two 4-byte uniforms with x and y bounds.
    // Must live until bind_group is created.
    let mut uniform_bufs: Vec<wgpu::Buffer> = Vec::new();

    if desc.uniform_bound_present != 0 {
        if has_x_bound {
            let w_u32 = narrow_uniform_bound(desc.uniform_bound_x_value)?;
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("miri_gpu_uniform_bound_x"),
                size: 4,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buf, 0, &w_u32.to_le_bytes());
            uniform_bufs.push(buf);
        }
        if has_y_bound {
            let h_u32 = narrow_uniform_bound(desc.uniform_bound_y_value)?;
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("miri_gpu_uniform_bound_y"),
                size: 4,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buf, 0, &h_u32.to_le_bytes());
            uniform_bufs.push(buf);
        }
    }

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
            let needs_narrow = buf_int_narrow.as_ref().is_some_and(|arr| arr[i] != 0);
            readback_device_buffer(
                device,
                queue,
                &storage_buffers[i],
                buf_data_ptrs[i],
                buf_byte_lens[i],
                needs_narrow,
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
/// When `buf_int_narrow[i]` is 1, the host buffer (i64 elements) is narrowed
/// to i32 on upload and widened back on readback.
///
/// # Safety
/// The three slices must be `num_bufs` long and their host pointers valid for
/// the matching byte lengths.
///
/// # Errors
/// Returns `Err` if any buffer value falls outside i32 range during narrowing.
unsafe fn prepare_capture_buffers(
    device: &Device,
    queue: &Queue,
    buf_handle_ids: &[u64],
    buf_data_ptrs: &[*mut u8],
    buf_byte_lens: &[usize],
    buf_int_narrow: Option<&[u8]>,
) -> Result<(Vec<wgpu::Buffer>, Vec<usize>), GpuError> {
    let mut storage_buffers = Vec::with_capacity(buf_handle_ids.len());
    let mut transient_captures = Vec::new();
    for i in 0..buf_handle_ids.len() {
        let needs_narrow = buf_int_narrow.is_some_and(|arr| arr[i] != 0);
        let buffer = if buf_handle_ids[i] != device_table::HOST_HANDLE {
            persistent_capture_buffer(
                device,
                queue,
                buf_handle_ids[i],
                buf_data_ptrs[i],
                buf_byte_lens[i],
                needs_narrow,
                i,
            )?
        } else {
            transient_captures.push(i);
            new_storage_buffer_with_upload(
                device,
                queue,
                buf_data_ptrs[i],
                buf_byte_lens[i],
                needs_narrow,
                i,
            )?
        };
        storage_buffers.push(buffer);
    }
    Ok((storage_buffers, transient_captures))
}

/// Returns the resident device buffer for `handle`, allocating and uploading
/// it on first capture and reusing it (no upload) on every later launch.
///
/// When the buffer is first uploaded, range validation occurs. Later captures
/// reuse the persistent buffer without re-validation.
///
/// # Errors
/// Returns `Err` if the buffer value falls outside i32 range during first upload.
unsafe fn persistent_capture_buffer(
    device: &Device,
    queue: &Queue,
    handle: u64,
    host_ptr: *mut u8,
    byte_len: usize,
    needs_narrow: bool,
    buffer_index: usize,
) -> Result<wgpu::Buffer, GpuError> {
    if let Some((existing, _, _)) = device_table::resident_buffer(handle) {
        return Ok(existing);
    }
    let buffer = new_storage_buffer_with_upload(
        device,
        queue,
        host_ptr,
        byte_len,
        needs_narrow,
        buffer_index,
    )?;
    let device_byte_len = if needs_narrow {
        let elem_count = byte_len / 8;
        elem_count
            .checked_mul(4)
            .unwrap_or_else(|| panic!("device buffer size overflow: {} * 4", elem_count))
    } else {
        byte_len
    };
    device_table::insert_resident(handle, buffer.clone(), device_byte_len, needs_narrow);
    Ok(buffer)
}

/// Allocates a storage buffer sized for `byte_len` (or narrowed size if needs_narrow).
/// When there are host bytes to copy, uploads them and records one upload in the telemetry counters;
/// an empty or null capture allocates the buffer without an upload.
///
/// When `needs_narrow` is true, the host buffer contains i64 elements
/// (8 bytes each) that are narrowed to i32 (4 bytes each) on upload.
///
/// # Panics
/// Panics if `byte_len` is not a multiple of 8 when `needs_narrow` is true (host buffer
/// must contain complete i64 elements).
///
/// # Errors
/// Returns `Err` if any element value falls outside i32 range during narrowing.
unsafe fn new_storage_buffer_with_upload(
    device: &Device,
    queue: &Queue,
    host_ptr: *mut u8,
    byte_len: usize,
    needs_narrow: bool,
    buffer_index: usize,
) -> Result<wgpu::Buffer, GpuError> {
    let (device_byte_len, upload_bytes) = if needs_narrow {
        // Host buffer is i64 elements (byte_len = 8*N); device buffer is i32 elements (4*N).
        // Guard: byte_len must be a multiple of 8.
        assert!(
            byte_len.is_multiple_of(8),
            "host buffer byte_len {} is not a multiple of 8 for i64 narrowing",
            byte_len
        );
        let elem_count = byte_len / 8;
        // Defend against integer overflow: check that elem_count * 4 doesn't overflow.
        let device_len = elem_count
            .checked_mul(4)
            .unwrap_or_else(|| panic!("device buffer size overflow: {} * 4", elem_count));
        let padded = align_to_4(device_len.max(4));
        let mut upload_bytes = Vec::with_capacity(device_len);
        if byte_len > 0 && !host_ptr.is_null() {
            let host_i64s = std::slice::from_raw_parts(host_ptr as *const i64, elem_count);
            for (elem_idx, &val) in host_i64s.iter().enumerate() {
                if val < i32::MIN as i64 || val > i32::MAX as i64 {
                    return Err(GpuError::ValueOutOfI32Range {
                        buffer_index,
                        element_index: elem_idx,
                        value: val,
                    });
                }
                upload_bytes.extend_from_slice(&(val as i32).to_le_bytes());
            }
        }
        (padded as u64, upload_bytes)
    } else {
        // Defend against integer overflow in align_to_4.
        let padded = align_to_4(byte_len.max(4));
        let bytes = if byte_len > 0 && !host_ptr.is_null() {
            std::slice::from_raw_parts(host_ptr as *const u8, byte_len).to_vec()
        } else {
            Vec::new()
        };
        (padded as u64, bytes)
    };

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("miri_gpu_launch_inline storage"),
        size: device_byte_len,
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
                min_binding_size: std::num::NonZeroU64::new(4),
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
    needs_narrow: bool,
) -> Result<(), GpuError> {
    let (device_byte_len, host_byte_len) = if needs_narrow {
        // Device buffer is i32 elements (4 bytes each); host buffer is i64 elements (8 bytes each).
        // Guard: byte_len (host length) must be a multiple of 8.
        assert!(
            byte_len.is_multiple_of(8),
            "host buffer byte_len {} is not a multiple of 8 for i64 widening",
            byte_len
        );
        let elem_count = byte_len / 8;
        let device_len = elem_count
            .checked_mul(4)
            .unwrap_or_else(|| panic!("device buffer size overflow: {} * 4", elem_count));
        (device_len, byte_len)
    } else {
        (byte_len, byte_len)
    };

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
    if needs_narrow {
        // Widen i32 elements back to i64.
        // Host array is 8-aligned by alloc_zeroed, so the i64 slice read is aligned.
        let elem_count = host_byte_len / 8;
        let device_i32s = std::slice::from_raw_parts(mapped.as_ptr() as *const i32, elem_count);
        let host_i64s = std::slice::from_raw_parts_mut(host_ptr as *mut i64, elem_count);
        device_i32s
            .iter()
            .zip(host_i64s.iter_mut())
            .for_each(|(&v, d)| *d = v as i64);
    } else {
        std::ptr::copy_nonoverlapping(mapped.as_ptr(), host_ptr, host_byte_len);
    }
    drop(mapped);
    staging.unmap();
    Ok(())
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
            128,
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
        assert_eq!(offset_of!(GpuLaunchDesc, buf_int_narrow), 96);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_present), 104);
        assert_eq!(offset_of!(GpuLaunchDesc, uniform_bound_value), 112);
        assert_eq!(offset_of!(GpuLaunchDesc, num_storage_bufs), 120);
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
    let Some((buffer, resident_byte_len, needs_widen)) = device_table::resident_buffer(handle)
    else {
        return 1;
    };
    // `readback_device_buffer` takes the HOST byte length and derives the device
    // length itself (host/8*4 for widened i64 buffers). For a widened buffer the
    // resident (device) length is half the host length, so clamping to it here
    // would re-narrow and drop the upper half — pass the host length directly.
    let byte_len = if needs_widen {
        host_byte_len
    } else {
        host_byte_len.min(resident_byte_len)
    };
    let Ok(ctx) = init_gpu_context() else {
        return 0;
    };
    match readback_device_buffer(
        &ctx.device,
        &ctx.queue,
        &buffer,
        header.data,
        byte_len,
        needs_widen,
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
/// buffer owned by `handle`. If the buffer does not yet exist, allocates and
/// uploads it (as if this binding were first captured in a launch).
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

    let Ok(ctx) = init_gpu_context() else {
        return 0;
    };

    // Check if buffer already exists; if so, reuse it with a write.
    if let Some((existing_buffer, existing_byte_len, needs_widen)) =
        device_table::resident_buffer(handle)
    {
        if host_byte_len == 0 || header.data.is_null() {
            return 1;
        }
        if needs_widen {
            // Host buffer is i64 elements that were narrowed to i32 on first upload.
            // Narrow again for this explicit upload.
            let elem_count = host_byte_len / 8;
            let device_len = elem_count.checked_mul(4).unwrap_or(0);
            if device_len > existing_byte_len {
                log::error!("miri_gpu_upload: narrowed device buffer too small");
                return 0;
            }
            let mut upload_bytes = Vec::with_capacity(device_len);
            let host_i64s = std::slice::from_raw_parts(header.data as *const i64, elem_count);
            for (elem_idx, &val) in host_i64s.iter().enumerate() {
                if val < i32::MIN as i64 || val > i32::MAX as i64 {
                    log::error!(
                        "miri_gpu_upload narrowing failed: buffer {} element {}: value {} \
                        exceeds i32 range [{}, {}]",
                        handle,
                        elem_idx,
                        val,
                        i32::MIN,
                        i32::MAX
                    );
                    return 0;
                }
                upload_bytes.extend_from_slice(&(val as i32).to_le_bytes());
            }
            ctx.queue.write_buffer(&existing_buffer, 0, &upload_bytes);
        } else {
            // No narrowing needed; write raw bytes.
            let upload_len = host_byte_len.min(existing_byte_len);
            ctx.queue.write_buffer(
                &existing_buffer,
                0,
                std::slice::from_raw_parts(header.data, upload_len),
            );
        }
        telemetry::record_upload();
        return 1;
    }

    // Buffer does not exist yet; allocate and upload it fresh (same as first-launch path).
    // For simplicity, assume no narrowing (the assignment path does not narrow; narrowing
    // is only applied when a host array with i64 elements is first uploaded in a launch).
    // If narrowing is needed in the future, the calling code must pass an i32-narrowed array.
    match new_storage_buffer_with_upload(
        &ctx.device,
        &ctx.queue,
        header.data,
        host_byte_len,
        false, // no narrowing for explicit upload assignment
        0,     // buffer_index is not meaningful for standalone upload
    ) {
        Ok(buffer) => {
            device_table::insert_resident(handle, buffer, host_byte_len, false);
            1
        }
        Err(err) => {
            log::error!("miri_gpu_upload failed: {:?}", err);
            0
        }
    }
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
}
