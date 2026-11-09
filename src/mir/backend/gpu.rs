// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU-specific MIR types.
//!
//! This module contains types that are specific to GPU backends (CUDA, Metal, SPIR-V, WebGPU).

use crate::ast::type_arguments::TypeArgument;
use crate::ast::types::TypeKind;
use crate::mir::Operand;
use std::fmt;

pub use crate::gpu_target::GpuAtomicOp;
/// How the type checker settled a `gpu` binding's device buffer from its
/// initializer — data the WebGPU backend reads to size and fill the buffer.
pub use crate::type_checker::GpuBufferInit;

/// The value of the constant integer expression `size` — the extent of a
/// fixed-size array a kernel declares — or `None` when it is not a constant.
///
/// Folded by the same evaluator the type checker requires such an extent to
/// satisfy, so a size the checker accepted is the size the backend declares.
pub fn fixed_array_extent(size: &TypeArgument) -> Option<i128> {
    crate::type_checker::TypeChecker::try_eval_const_argument(size, None)
}

/// How a GPU array-index value must be narrowed to the backend's native
/// 32-bit index width.
///
/// GPU shader index types are 32-bit (`i32`/`u32` in WGSL, `OpTypeInt 32` in
/// SPIR-V, `int` in PTX). The narrowing a given index needs depends only on its
/// scalar type, so the decision is backend-neutral; each backend renders the
/// chosen narrowing in its own syntax. Centralizing the policy here lets every
/// GPU backend share one source of truth for index narrowing.
///
/// A device computes `int` and `i64` in a 32-bit lane, and stops the program
/// on a result that does not fit it, so a cast to the 32-bit index type keeps
/// the index's value. Whether that value lies inside the collection is the
/// index check's question, not the narrowing's: an index nothing proved in
/// range is checked, and one outside records a fault rather than being
/// clamped into an element it does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuIndexNarrowing {
    /// Index is `Int` or `I64`: emit a 32-bit cast.
    Identity,
    /// Index needs no narrowing: emit it unchanged.
    None,
}

impl GpuIndexNarrowing {
    /// Classifies the narrowing a GPU array index of the given scalar type needs.
    pub fn from_index_kind(index_kind: &TypeKind) -> Self {
        match index_kind {
            TypeKind::I64 | TypeKind::Int => GpuIndexNarrowing::Identity,
            _ => GpuIndexNarrowing::None,
        }
    }
}

/// GPU-specific function metadata.
///
/// Attached to `Body` via `BackendMetadata::Gpu` for GPU kernels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GpuBodyMetadata {
    /// Compile-time workgroup/block size.
    /// Required for WebGPU/SPIR-V compute shaders, optional for CUDA/Metal.
    /// Format: [x, y, z]
    pub workgroup_size: Option<[u32; 3]>,
    /// Dispatch grid size (number of workgroups).
    /// Computed statically for `forall` with literal loop bounds.
    /// For runtime-bound loops, this may be None (grid computed at runtime).
    /// Format: [x, y, z]
    pub grid_size: Option<[u32; 3]>,
    /// Logical per-axis iteration extent (the loop lengths themselves, NOT the
    /// block-rounded dispatch grid). Set for a `forall` with literal bounds; used
    /// by the web-gpu backend to recover a rectangular canvas from a 2-D paint
    /// kernel (grid_size alone is block-rounded and loses the exact extent).
    /// Format: [x, y, z].
    pub logical_extent: Option<[u32; 3]>,
    /// Required GPU capabilities for this kernel.
    pub required_capabilities: Vec<GpuCapability>,
    /// True if this kernel is a frame-step animation kernel.
    /// Frame-step kernels are marked by `gpu frame` and are dispatched every animation frame
    /// in web-gpu bundles. Only one frame-step kernel per program is supported.
    pub is_frame_step: bool,
}

/// GPU hardware capabilities that may be required by a kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuCapability {
    /// Shared memory (workgroup-local memory)
    SharedMemory,
    /// 32-bit integer atomics
    AtomicInt32,
    /// 64-bit integer atomics
    AtomicInt64,
    /// Floating-point atomics
    AtomicFloat,
    /// Subgroup/warp operations
    SubgroupOperations,
}

/// Argument passed to a GPU kernel via `GpuLaunch`.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuKernelArg {
    /// The operand being passed to the kernel.
    pub operand: Operand,
    /// Binding information for shader APIs (SPIR-V, WebGPU, Metal).
    pub binding: Option<GpuBinding>,
    /// Memory access pattern for this argument.
    pub access: GpuMemoryAccess,
}

/// Binding location for GPU buffer arguments.
///
/// Used by SPIR-V, WebGPU, and Metal to specify where an argument is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuBinding {
    /// Descriptor set (SPIR-V) / argument buffer index (Metal).
    pub set: u32,
    /// Binding index within the set.
    pub binding: u32,
}

/// Memory access pattern for GPU arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GpuMemoryAccess {
    /// Read-only access
    #[default]
    Read,
    /// Write-only access
    Write,
    /// Read-write access
    ReadWrite,
}

impl fmt::Display for GpuMemoryAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuMemoryAccess::Read => write!(f, "read"),
            GpuMemoryAccess::Write => write!(f, "write"),
            GpuMemoryAccess::ReadWrite => write!(f, "read_write"),
        }
    }
}

/// Memory barrier scope for GPU synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuMemoryScope {
    /// Synchronize within a workgroup/block
    /// - CUDA: __syncthreads()
    /// - Metal: threadgroup_barrier()
    /// - SPIR-V: Workgroup scope
    Workgroup,
    /// Synchronize across entire device
    /// - CUDA: __threadfence()
    /// - Metal: device memory fence
    /// - SPIR-V: Device scope
    Device,
}

impl fmt::Display for GpuMemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuMemoryScope::Workgroup => write!(f, "workgroup"),
            GpuMemoryScope::Device => write!(f, "device"),
        }
    }
}

/// Backend-specific function metadata.
///
/// This enum allows `Body` to carry metadata for different backend types
/// without polluting the core MIR types with backend-specific fields.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendMetadata {
    /// GPU-specific metadata
    Gpu(GpuBodyMetadata),
    // Future backends can be added here:
    // Tpu(TpuBodyMetadata),
    // Fpga(FpgaBodyMetadata),
}
