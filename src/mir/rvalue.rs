// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use std::fmt;

/// Kind of aggregate being constructed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AggregateKind {
    /// A tuple, e.g., `(1, "hello", true)` - fixed size, heterogeneous
    Tuple,
    /// An array, e.g., `[1, 2, 3; 3]` - fixed size, homogeneous
    Array,
    /// A struct, e.g., `Point { x: 1, y: 2 }` - named fields
    Struct(Type),
    /// A list, e.g., `[1, 2, 3]` - dynamic size, homogeneous
    List,
    /// A set, e.g., `{1, 2, 3}` - dynamic size, unique elements
    Set,
    /// A map, e.g., `{"a": 1, "b": 2}` - key-value pairs
    /// Operands alternate: key1, val1, key2, val2, ...
    Map,
}

/// Right-hand value: the result of a computation.
///
/// An `Rvalue` produces a value that can be assigned to a `Place`.
/// Unlike operands, rvalues represent computations (operations, references, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rvalue {
    /// Use the operand as is (copy or move).
    Use(Operand),
    /// Create a reference to a place.
    Ref(Place),
    /// Binary operation.
    BinaryOp(BinOp, Box<Operand>, Box<Operand>),
    /// Unary operation.
    UnaryOp(UnOp, Box<Operand>),
    /// Cast operand to a type.
    Cast(Box<Operand>, Type),
    /// Get the length of an array/slice.
    Len(Place),
    /// GPU intrinsic operation (thread index, block index, etc.)
    GpuIntrinsic(GpuIntrinsic),
    /// Construct an aggregate value from operands.
    /// - Tuple: operands are tuple elements in order
    /// - Array/List: operands are elements in order
    /// - Set: operands are unique elements
    /// - Map: operands alternate key1, val1, key2, val2, ...
    /// - Struct: operands are field values in declaration order
    Aggregate(AggregateKind, Vec<Operand>),
}

impl fmt::Display for Rvalue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rvalue::Use(op) => write!(f, "{}", op),
            Rvalue::Ref(place) => write!(f, "&{}", place),
            Rvalue::BinaryOp(op, lhs, rhs) => write!(f, "{:?}({}, {})", op, lhs, rhs),
            Rvalue::UnaryOp(op, val) => write!(f, "{:?}({})", op, val),
            Rvalue::Cast(op, ty) => write!(f, "{} as {}", op, ty),
            Rvalue::Len(place) => write!(f, "Len({})", place),
            Rvalue::GpuIntrinsic(intrinsic) => write!(f, "{}", intrinsic),
            Rvalue::Aggregate(kind, ops) => match kind {
                AggregateKind::Tuple => {
                    write!(f, "(")?;
                    for (i, op) in ops.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", op)?;
                    }
                    write!(f, ")")
                }
                AggregateKind::Array | AggregateKind::List => {
                    write!(f, "[")?;
                    for (i, op) in ops.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", op)?;
                    }
                    write!(f, "]")
                }
                AggregateKind::Set => {
                    write!(f, "{{")?;
                    for (i, op) in ops.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", op)?;
                    }
                    write!(f, "}}")
                }
                AggregateKind::Map => {
                    write!(f, "{{")?;
                    for (i, chunk) in ops.chunks(2).enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        if chunk.len() == 2 {
                            write!(f, "{}: {}", chunk[0], chunk[1])?;
                        }
                    }
                    write!(f, "}}")
                }
                AggregateKind::Struct(ty) => {
                    write!(f, "{} {{ ", ty)?;
                    for (i, op) in ops.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", op)?;
                    }
                    write!(f, " }}")
                }
            },
        }
    }
}

/// GPU-specific intrinsic operations.
///
/// These are low-level operations that map directly to GPU hardware concepts.
/// Backend mappings:
///
/// | Intrinsic    | CUDA            | Metal                    | SPIR-V              | WebGPU (WGSL)           |
/// |--------------|-----------------|--------------------------|---------------------|-------------------------|
/// | ThreadIdx    | threadIdx       | thread_position_in_threadgroup | GlobalInvocationId* | global_invocation_id*   |
/// | BlockIdx     | blockIdx        | threadgroup_position_in_grid | WorkgroupId         | workgroup_id            |
/// | BlockDim     | blockDim        | threads_per_threadgroup  | WorkgroupSize       | workgroup_size (const)  |
/// | GridDim      | gridDim         | threadgroups_per_grid    | NumWorkgroups       | num_workgroups          |
/// | SyncThreads  | __syncthreads() | threadgroup_barrier()    | OpControlBarrier    | workgroupBarrier()      |
///
/// *Note: SPIR-V/WGSL use flat global IDs by default. ThreadIdx requires computing
/// `global_id - workgroup_id * workgroup_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuIntrinsic {
    /// Thread index within a block.
    /// - CUDA: `threadIdx.x/y/z`
    /// - Metal: `thread_position_in_threadgroup`
    /// - SPIR-V: Computed from `GlobalInvocationId - WorkgroupId * WorkgroupSize`
    /// - WebGPU: Computed from `global_invocation_id - workgroup_id * workgroup_size`
    ThreadIdx(Dimension),
    /// Block/workgroup index within the grid.
    /// - CUDA: `blockIdx.x/y/z`
    /// - Metal: `threadgroup_position_in_grid`
    /// - SPIR-V: `WorkgroupId`
    /// - WebGPU: `workgroup_id`
    BlockIdx(Dimension),
    /// Number of threads per block/workgroup.
    /// - CUDA: `blockDim.x/y/z`
    /// - Metal: `threads_per_threadgroup`
    /// - SPIR-V: `WorkgroupSize` (specialization constant)
    /// - WebGPU: `workgroup_size` (compile-time constant via `@workgroup_size`)
    BlockDim(Dimension),
    /// Number of blocks/workgroups in the grid.
    /// - CUDA: `gridDim.x/y/z`
    /// - Metal: `threadgroups_per_grid`
    /// - SPIR-V: `NumWorkgroups`
    /// - WebGPU: `num_workgroups`
    GridDim(Dimension),
    /// Synchronize all threads within a block/workgroup.
    /// - CUDA: `__syncthreads()`
    /// - Metal: `threadgroup_barrier(mem_flags::mem_threadgroup)`
    /// - SPIR-V: `OpControlBarrier` with Workgroup scope
    /// - WebGPU: `workgroupBarrier()`
    SyncThreads,
}

impl fmt::Display for GpuIntrinsic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuIntrinsic::ThreadIdx(d) => write!(f, "gpu_thread_idx.{}", d),
            GpuIntrinsic::BlockIdx(d) => write!(f, "gpu_block_idx.{}", d),
            GpuIntrinsic::BlockDim(d) => write!(f, "gpu_block_dim.{}", d),
            GpuIntrinsic::GridDim(d) => write!(f, "gpu_grid_dim.{}", d),
            GpuIntrinsic::SyncThreads => write!(f, "gpu_sync_threads"),
        }
    }
}

/// Dimension index for GPU intrinsics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    X = 0,
    Y = 1,
    Z = 2,
}

impl Dimension {
    /// Create a Dimension from a numeric index (0=X, 1=Y, 2=Z).
    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Dimension::X),
            1 => Some(Dimension::Y),
            2 => Some(Dimension::Z),
            _ => None,
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dimension::X => write!(f, "x"),
            Dimension::Y => write!(f, "y"),
            Dimension::Z => write!(f, "z"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Offset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Not,
    Neg,
}
