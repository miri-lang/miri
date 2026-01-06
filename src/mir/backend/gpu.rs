// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! GPU-specific MIR types.
//!
//! This module contains types that are specific to GPU backends (CUDA, Metal, SPIR-V, WebGPU).

use crate::mir::Operand;
use std::fmt;

/// GPU-specific function metadata.
///
/// Attached to `Body` via `BackendMetadata::Gpu` for GPU kernels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GpuBodyMetadata {
    /// Compile-time workgroup/block size.
    /// Required for WebGPU/SPIR-V compute shaders, optional for CUDA/Metal.
    /// Format: [x, y, z]
    pub workgroup_size: Option<[u32; 3]>,
    /// Required GPU capabilities for this kernel.
    pub required_capabilities: Vec<GpuCapability>,
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

/// Atomic operation types for GPU memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuAtomicOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Min,
    Max,
    Exchange,
    CompareExchange,
}

impl fmt::Display for GpuAtomicOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuAtomicOp::Add => write!(f, "add"),
            GpuAtomicOp::Sub => write!(f, "sub"),
            GpuAtomicOp::And => write!(f, "and"),
            GpuAtomicOp::Or => write!(f, "or"),
            GpuAtomicOp::Xor => write!(f, "xor"),
            GpuAtomicOp::Min => write!(f, "min"),
            GpuAtomicOp::Max => write!(f, "max"),
            GpuAtomicOp::Exchange => write!(f, "exchange"),
            GpuAtomicOp::CompareExchange => write!(f, "compare_exchange"),
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
