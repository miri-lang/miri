//! High-level `gpu for` launch helper.
//!
//! `miri_gpu_launch_inline` is the single FFI entry Cranelift emits at
//! each `TerminatorKind::GpuLaunch`. It bundles init / compile / cache
//! / dispatch / sync / readback so the compiler can stay backend-agnostic
//! about wgpu specifics.
//!
//! Kernel compilation is cached by name in the existing `KernelRegistry`
//! so repeated dispatches of the same kernel pay the compile cost once.

use crate::compute::{get_kernel_by_name, CompiledKernel};
use crate::context::{init_gpu_context, GpuContext, GpuError};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::sync::Arc;
use wgpu::{BufferUsages, Device, Features, Queue};

/// Routes through the shared `context::GPU_CONTEXT` so a successful
/// `gpu for` dispatch makes `miri_gpu_is_available()` (and therefore
/// `system.gpu.is_gpu_available()`) start returning true. Keeping a
/// separate `OnceCell` here would leave the public probe permanently
/// false even after the runtime has booted a device.
fn ensure_context() -> Result<&'static Arc<GpuContext>, GpuError> {
    init_gpu_context()
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
}

/// Launches a GPU kernel inline. Returns 1 on success, 0 on failure.
///
/// On success, every capture buffer pointed to by `buf_data_ptrs[i]` is
/// overwritten with the post-dispatch device contents (baseline assumes
/// every capture is read/write, matching the current `gpu for` lowering).
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
            log::error!("miri_gpu_launch_inline failed: {:?}", err);
            0
        }
    }
}

unsafe fn launch_impl(desc: &GpuLaunchDesc) -> Result<(), GpuError> {
    let wgsl = decode_utf8(desc.wgsl_ptr, desc.wgsl_len)?;
    let entry_point = decode_utf8(desc.entry_ptr, desc.entry_len)?;

    let ctx = ensure_context()?;
    check_required_shader_features(wgsl, ctx.enabled_shader_features)?;

    let kernel = ensure_kernel(
        entry_point,
        wgsl,
        desc.num_bufs,
        [desc.block_x, desc.block_y, desc.block_z],
    )?;

    let device = &ctx.device;
    let queue = &ctx.queue;

    let buf_data_ptrs = std::slice::from_raw_parts(desc.buf_data_ptrs, desc.num_bufs);
    let buf_byte_lens = std::slice::from_raw_parts(desc.buf_byte_lens, desc.num_bufs);

    let storage_buffers: Vec<wgpu::Buffer> = (0..desc.num_bufs)
        .map(|i| {
            let host_ptr = buf_data_ptrs[i];
            let byte_len = buf_byte_lens[i];
            let padded = align_to_4(byte_len.max(4));
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("miri_gpu_launch_inline storage"),
                size: padded as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if byte_len > 0 && !host_ptr.is_null() {
                let bytes = std::slice::from_raw_parts(host_ptr as *const u8, byte_len);
                queue.write_buffer(&buffer, 0, bytes);
            }
            buffer
        })
        .collect();

    let entries: Vec<wgpu::BindGroupEntry> = storage_buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
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
    device.poll(wgpu::Maintain::Wait);

    for i in 0..desc.num_bufs {
        let byte_len = buf_byte_lens[i];
        let host_ptr = buf_data_ptrs[i];
        if byte_len == 0 || host_ptr.is_null() {
            continue;
        }
        readback_into_host(device, queue, &storage_buffers[i], host_ptr, byte_len)?;
    }
    Ok(())
}

/// Refuse to dispatch a kernel whose WGSL references a 64-bit scalar
/// (`i64`/`u64`/`f64`) when the device was not booted with the matching
/// wgpu feature. Without this gate, naga's shader-module compilation would
/// reject the kernel later with a generic message; surfacing the cause
/// upfront keeps the diagnostic source-relevant (which scalar) instead of
/// pipeline-relevant (which wgpu validator rule fired).
fn check_required_shader_features(wgsl: &str, enabled: Features) -> Result<(), GpuError> {
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
fn wgsl_uses_scalar(wgsl: &str, name: &str) -> bool {
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

fn ensure_kernel(
    entry_point: &str,
    wgsl: &str,
    num_bindings: usize,
    workgroup_size: [u32; 3],
) -> Result<Arc<CompiledKernel>, GpuError> {
    let key = cache_key(entry_point, wgsl);
    if let Some(existing) = get_kernel_by_name(&key) {
        return Ok(existing);
    }
    compile_and_register(entry_point, &key, wgsl, num_bindings, workgroup_size)
}

fn compile_and_register(
    entry_point: &str,
    cache_name: &str,
    wgsl: &str,
    num_bindings: usize,
    workgroup_size: [u32; 3],
) -> Result<Arc<CompiledKernel>, GpuError> {
    static REGISTER_LOCK: OnceCell<RwLock<()>> = OnceCell::new();
    let lock = REGISTER_LOCK.get_or_init(|| RwLock::new(()));
    let _guard = lock.write();
    if let Some(existing) = get_kernel_by_name(cache_name) {
        return Ok(existing);
    }
    let kernel =
        compile_kernel_inline(entry_point, cache_name, wgsl, num_bindings, workgroup_size)?;
    let id = kernel.id;
    crate::compute::register_kernel_inline(kernel);
    get_kernel_by_id(id).ok_or_else(|| {
        GpuError::ShaderCompilationFailed(format!("failed to register {}", cache_name))
    })
}

fn get_kernel_by_id(id: u64) -> Option<Arc<CompiledKernel>> {
    crate::compute::get_kernel(id)
}

fn compile_kernel_inline(
    entry_point: &str,
    cache_name: &str,
    wgsl: &str,
    num_bindings: usize,
    workgroup_size: [u32; 3],
) -> Result<CompiledKernel, GpuError> {
    let ctx = ensure_context()?;
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(cache_name),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
    let bind_group_layout = build_bind_group_layout(&ctx.device, cache_name, num_bindings);
    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{}_pl", cache_name)),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&format!("{}_pipeline", cache_name)),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point,
            compilation_options: Default::default(),
        });
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

fn build_bind_group_layout(
    device: &Device,
    name: &str,
    num_bindings: usize,
) -> wgpu::BindGroupLayout {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..num_bindings)
        .map(|i| wgpu::BindGroupLayoutEntry {
            binding: i as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
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

unsafe fn readback_into_host(
    device: &Device,
    queue: &Queue,
    src: &wgpu::Buffer,
    host_ptr: *mut u8,
    byte_len: usize,
) -> Result<(), GpuError> {
    let padded = align_to_4(byte_len.max(4)) as u64;
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
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|_| GpuError::BufferCreationFailed)?
        .map_err(|_| GpuError::BufferCreationFailed)?;

    let mapped = slice.get_mapped_range();
    std::ptr::copy_nonoverlapping(mapped.as_ptr(), host_ptr, byte_len);
    drop(mapped);
    staging.unmap();
    Ok(())
}

fn align_to_4(value: usize) -> usize {
    (value + 3) & !3
}
