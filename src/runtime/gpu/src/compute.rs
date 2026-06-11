// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Compute kernel compilation and dispatch.
//!
//! Kernels are compiled from WGSL source produced by Miri's WGSL
//! backend. Each kernel is registered by name + numeric ID; FFI calls
//! reference them by handle.

use crate::buffer::get_buffer;
use crate::context::{get_gpu_context, with_validation_scope, GpuError};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BufferBindingType, ComputePipeline,
    PipelineLayoutDescriptor, ShaderModule, ShaderStages,
};

static NEXT_KERNEL_ID: AtomicU64 = AtomicU64::new(1);

static KERNEL_REGISTRY: Lazy<RwLock<HashMap<u64, Arc<CompiledKernel>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static KERNEL_NAME_REGISTRY: Lazy<RwLock<HashMap<String, u64>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub struct CompiledKernel {
    pub id: u64,
    pub name: String,
    pub shader_module: ShaderModule,
    pub pipeline: ComputePipeline,
    pub bind_group_layout: BindGroupLayout,
    pub num_bindings: usize,
    pub workgroup_size: [u32; 3],
}

#[repr(C)]
pub struct KernelHandle {
    pub id: u64,
    pub num_bindings: usize,
    pub workgroup_size: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DispatchSize {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl Default for DispatchSize {
    fn default() -> Self {
        Self { x: 1, y: 1, z: 1 }
    }
}

impl CompiledKernel {
    pub fn from_wgsl(
        name: &str,
        source: &str,
        entry_point: &str,
        num_bindings: usize,
        workgroup_size: [u32; 3],
    ) -> Result<Self, GpuError> {
        let ctx = get_gpu_context()?;

        let (shader_module, bind_group_layout, pipeline) =
            with_validation_scope(&ctx.device, || {
                let shader_module = ctx
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(name),
                        source: wgpu::ShaderSource::Wgsl(source.into()),
                    });
                let bind_group_layout = build_bind_group_layout(ctx, name, num_bindings);
                let pipeline_layout =
                    ctx.device
                        .create_pipeline_layout(&PipelineLayoutDescriptor {
                            label: Some(&format!("{}_pipeline_layout", name)),
                            bind_group_layouts: &[Some(&bind_group_layout)],
                            immediate_size: 0,
                        });
                let pipeline =
                    ctx.device
                        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some(&format!("{}_pipeline", name)),
                            layout: Some(&pipeline_layout),
                            module: &shader_module,
                            entry_point: Some(entry_point),
                            compilation_options: Default::default(),
                            cache: None,
                        });

                (shader_module, bind_group_layout, pipeline)
            })?;

        let id = NEXT_KERNEL_ID.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            id,
            name: name.to_string(),
            shader_module,
            pipeline,
            bind_group_layout,
            num_bindings,
            workgroup_size,
        })
    }

    pub fn create_bind_group(&self, buffer_ids: &[u64]) -> Result<BindGroup, GpuError> {
        if buffer_ids.len() != self.num_bindings {
            return Err(GpuError::InvalidDimensions);
        }
        let ctx = get_gpu_context()?;
        let buffers: Result<Vec<_>, _> = buffer_ids
            .iter()
            .map(|&id| get_buffer(id).ok_or(GpuError::NotInitialized))
            .collect();
        let buffers = buffers?;
        let entries: Vec<BindGroupEntry> = buffers
            .iter()
            .enumerate()
            .map(|(i, buf)| BindGroupEntry {
                binding: i as u32,
                resource: buf.buffer.as_entire_binding(),
            })
            .collect();
        let bind_group = ctx.device.create_bind_group(&BindGroupDescriptor {
            label: Some(&format!("{}_bind_group", self.name)),
            layout: &self.bind_group_layout,
            entries: &entries,
        });
        Ok(bind_group)
    }

    pub fn dispatch(&self, buffer_ids: &[u64], dispatch: DispatchSize) -> Result<(), GpuError> {
        let ctx = get_gpu_context()?;
        let bind_group = self.create_bind_group(buffer_ids)?;
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(&format!("{}_encoder", self.name)),
            });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(&format!("{}_pass", self.name)),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(dispatch.x, dispatch.y, dispatch.z);
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }
}

fn build_bind_group_layout(
    ctx: &crate::context::GpuContext,
    name: &str,
    num_bindings: usize,
) -> BindGroupLayout {
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
            label: Some(&format!("{}_layout", name)),
            entries: &entries,
        })
}

pub fn get_kernel(id: u64) -> Option<Arc<CompiledKernel>> {
    KERNEL_REGISTRY.read().get(&id).cloned()
}

pub fn get_kernel_by_name(name: &str) -> Option<Arc<CompiledKernel>> {
    let id = KERNEL_NAME_REGISTRY.read().get(name).copied()?;
    get_kernel(id)
}

fn register_kernel(kernel: CompiledKernel) -> Arc<CompiledKernel> {
    let id = kernel.id;
    let name = kernel.name.clone();
    let arc = Arc::new(kernel);
    KERNEL_REGISTRY.write().insert(id, Arc::clone(&arc));
    KERNEL_NAME_REGISTRY.write().insert(name, id);
    arc
}

/// Internal: register a kernel built by another module (`launch::compile_and_register`).
pub fn register_kernel_inline(kernel: CompiledKernel) {
    register_kernel(kernel);
}

fn remove_kernel(id: u64) {
    if let Some(kernel) = KERNEL_REGISTRY.write().remove(&id) {
        KERNEL_NAME_REGISTRY.write().remove(&kernel.name);
    }
}

fn handle_from(kernel: &CompiledKernel) -> KernelHandle {
    KernelHandle {
        id: kernel.id,
        num_bindings: kernel.num_bindings,
        workgroup_size: kernel.workgroup_size,
    }
}

/// # Safety
/// All three name pointers must point to UTF-8 byte ranges of their
/// stated lengths.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_kernel_compile_wgsl(
    name: *const u8,
    name_len: usize,
    source: *const u8,
    source_len: usize,
    entry_point: *const u8,
    entry_len: usize,
    num_bindings: usize,
    workgroup_x: u32,
    workgroup_y: u32,
    workgroup_z: u32,
) -> *mut KernelHandle {
    if name.is_null() || source.is_null() || entry_point.is_null() {
        return ptr::null_mut();
    }
    let Ok(name_str) = std::str::from_utf8(std::slice::from_raw_parts(name, name_len)) else {
        return ptr::null_mut();
    };
    let Ok(source_str) = std::str::from_utf8(std::slice::from_raw_parts(source, source_len)) else {
        return ptr::null_mut();
    };
    let Ok(entry_str) = std::str::from_utf8(std::slice::from_raw_parts(entry_point, entry_len))
    else {
        return ptr::null_mut();
    };
    let workgroup_size = [workgroup_x, workgroup_y, workgroup_z];
    match CompiledKernel::from_wgsl(
        name_str,
        source_str,
        entry_str,
        num_bindings,
        workgroup_size,
    ) {
        Ok(kernel) => {
            let handle = handle_from(&kernel);
            register_kernel(kernel);
            Box::into_raw(Box::new(handle))
        }
        Err(_) => ptr::null_mut(),
    }
}

/// # Safety
/// `name` must point to a UTF-8 byte range of length `name_len`.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_kernel_get(
    name: *const u8,
    name_len: usize,
) -> *mut KernelHandle {
    if name.is_null() {
        return ptr::null_mut();
    }
    let Ok(name_str) = std::str::from_utf8(std::slice::from_raw_parts(name, name_len)) else {
        return ptr::null_mut();
    };
    match get_kernel_by_name(name_str) {
        Some(kernel) => Box::into_raw(Box::new(handle_from(&kernel))),
        None => ptr::null_mut(),
    }
}

/// # Safety
/// `kernel` must be a valid `KernelHandle` pointer and `buffer_ids`
/// must point to at least `num_buffers` `u64` values.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_kernel_launch(
    kernel: *const KernelHandle,
    buffer_ids: *const u64,
    num_buffers: usize,
    dispatch_x: u32,
    dispatch_y: u32,
    dispatch_z: u32,
) -> u8 {
    if kernel.is_null() || buffer_ids.is_null() {
        return 0;
    }
    let Some(kernel_arc) = get_kernel((*kernel).id) else {
        return 0;
    };
    if num_buffers != kernel_arc.num_bindings {
        return 0;
    }
    let buffer_ids_slice = std::slice::from_raw_parts(buffer_ids, num_buffers);
    let dispatch = DispatchSize {
        x: dispatch_x,
        y: dispatch_y,
        z: dispatch_z,
    };
    u8::from(kernel_arc.dispatch(buffer_ids_slice, dispatch).is_ok())
}

/// # Safety
/// `kernel` must be a valid `KernelHandle` pointer previously returned
/// from a registration call.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_kernel_handle_free(kernel: *mut KernelHandle) {
    if !kernel.is_null() {
        let _ = Box::from_raw(kernel);
    }
}

/// # Safety
/// `kernel` must be a valid `KernelHandle` pointer previously returned
/// from a registration call.
#[no_mangle]
pub unsafe extern "C" fn miri_gpu_kernel_unload(kernel: *mut KernelHandle) {
    if !kernel.is_null() {
        let id = (*kernel).id;
        remove_kernel(id);
        let _ = Box::from_raw(kernel);
    }
}

#[cfg(test)]
mod shader_compilation_tests {
    use super::*;

    /// Test that invalid WGSL in CompiledKernel::from_wgsl returns
    /// ShaderCompilationFailed instead of panicking.
    #[test]
    fn from_wgsl_invalid_returns_error() {
        let _ctx = match crate::context::get_gpu_context() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("No GPU adapter available, skipping from_wgsl_invalid_returns_error");
                return;
            }
        };

        // Invalid WGSL: references undeclared variable.
        let invalid_wgsl = r#"
            @compute @workgroup_size(256)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                var x = undefined_variable;
            }
        "#;

        let result =
            CompiledKernel::from_wgsl("invalid_kernel", invalid_wgsl, "main", 0, [256, 1, 1]);

        match result {
            Err(GpuError::ShaderCompilationFailed(msg)) => {
                let msg_lower = msg.to_lowercase();
                // Assert the message mentions either the undefined variable or a validation keyword.
                // naga reports validation/scope errors; exact phrasing is version-dependent.
                assert!(
                    msg_lower.contains("undefined_variable")
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
}
