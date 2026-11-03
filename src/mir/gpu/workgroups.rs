// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The workgroup size each GPU kernel runs with, taken from its launches.

use std::collections::HashMap;

use crate::mir::backend::{BackendMetadata, GpuBodyMetadata};
use crate::mir::symbol::Symbol;
use crate::mir::{Body, ExecutionModel};

/// Why a kernel's workgroup size cannot be settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkgroupError {
    /// Two launches of the kernel named `kernel` in WGSL ask for different
    /// workgroup shapes.
    Conflicting {
        kernel: String,
        first: [u32; 3],
        second: [u32; 3],
    },
    /// A launch names a kernel, by its WGSL name, that no compiled kernel
    /// body carries.
    NoKernelBody(String),
}

impl std::fmt::Display for WorkgroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflicting {
                kernel,
                first,
                second,
            } => write!(
                f,
                "conflicting launch workgroup shapes for kernel '{}': \
                 {} {} {} vs {} {} {} (per-shape monomorphization is deferred)",
                kernel, first[0], first[1], first[2], second[0], second[1], second[2]
            ),
            Self::NoKernelBody(kernel) => write!(
                f,
                "a launch records a workgroup size for the kernel '{}', which has no \
                 compiled kernel body",
                kernel
            ),
        }
    }
}

/// Apply recorded GPU kernel workgroup sizes to the corresponding GPU kernel bodies.
///
/// When a `kernel(args).launch(grid, block)` call is lowered in a caller's body,
/// the block size is recorded in that body's `kernel_workgroups` vec.
/// This pass collects all these recordings across all bodies and applies each
/// to the corresponding GPU kernel body by setting its `BackendMetadata::Gpu.workgroup_size`.
///
/// If a kernel has multiple conflicting block sizes recorded (different launches
/// with different blocks), returns an error — per-shape monomorphization is deferred.
pub fn stamp_kernel_workgroups(bodies: &mut [(Symbol, Body)]) -> Result<(), WorkgroupError> {
    // Collect all workgroup size recordings from all bodies.
    let mut workgroup_map: HashMap<Symbol, [u32; 3]> = HashMap::new();

    for (_name, body) in bodies.iter() {
        for (kernel, block_size) in &body.kernel_workgroups {
            if let Some(&existing) = workgroup_map.get(kernel) {
                if existing != *block_size {
                    return Err(WorkgroupError::Conflicting {
                        kernel: kernel.wgsl_name(),
                        first: existing,
                        second: *block_size,
                    });
                }
            } else {
                workgroup_map.insert(kernel.clone(), *block_size);
            }
        }
    }

    // Apply each workgroup size to the corresponding kernel body. A helper's
    // symbol also names its CPU original, so the kernel is selected by its
    // execution model as well.
    for (kernel, block_size) in workgroup_map {
        let Some((_, body)) = bodies.iter_mut().find(|(symbol, body)| {
            *symbol == kernel && body.execution_model == ExecutionModel::GpuKernel
        }) else {
            return Err(WorkgroupError::NoKernelBody(kernel.wgsl_name()));
        };
        match &mut body.backend_metadata {
            Some(BackendMetadata::Gpu(gpu_md)) => gpu_md.workgroup_size = Some(block_size),
            None => {
                body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
                    workgroup_size: Some(block_size),
                    ..Default::default()
                }));
            }
        }
    }

    Ok(())
}
