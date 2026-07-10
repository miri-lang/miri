// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU-specific MIR types.
//!
//! This module contains types that are specific to GPU backends (CUDA, Metal, SPIR-V, WebGPU).

use crate::ast::types::TypeKind;
use crate::mir::Operand;
use std::fmt;

/// Largest non-negative value representable as a signed 32-bit GPU index.
///
/// A 64-bit index is saturated into `[0, I32_INDEX_MAX]` before being narrowed
/// to the backend's native 32-bit index width.
pub const I32_INDEX_MAX: i32 = i32::MAX;

/// How a GPU array-index value must be narrowed to the backend's native
/// 32-bit index width.
///
/// GPU shader index types are 32-bit (`i32`/`u32` in WGSL, `OpTypeInt 32` in
/// SPIR-V, `int` in PTX). The narrowing a given index needs depends only on its
/// scalar type, so the decision is backend-neutral; each backend renders the
/// chosen narrowing in its own syntax. Centralizing the policy here lets every
/// GPU backend share one source of truth for index narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuIndexNarrowing {
    /// Index is already 32-bit (`Int`): emit an identity 32-bit cast.
    Identity,
    /// Index is 64-bit (`I64`): saturate into `[0, I32_INDEX_MAX]` before the
    /// 32-bit cast, so a value `>= 2^31` cannot wrap into an aliasing in-bounds
    /// index.
    SaturateToI32,
    /// Index needs no narrowing: emit it unchanged.
    None,
}

impl GpuIndexNarrowing {
    /// Classifies the narrowing a GPU array index of the given scalar type needs.
    pub fn from_index_kind(index_kind: &TypeKind) -> Self {
        match index_kind {
            TypeKind::I64 => GpuIndexNarrowing::SaturateToI32,
            TypeKind::Int => GpuIndexNarrowing::Identity,
            _ => GpuIndexNarrowing::None,
        }
    }
}

/// Why a surface identifier collides with a WGSL reserved form and would be
/// rejected by the shader compiler if emitted verbatim as a WGSL name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgslNameConflict {
    /// WGSL reserves every identifier that begins with two underscores.
    DoubleUnderscorePrefix,
    /// The identifier is a WGSL keyword or reserved word.
    ReservedWord,
}

impl WgslNameConflict {
    /// A human-readable phrase describing the conflict, for a diagnostic that
    /// reads `GPU function name '<name>' <describe()>`.
    pub fn describe(self) -> &'static str {
        match self {
            WgslNameConflict::DoubleUnderscorePrefix => {
                "begins with a reserved double-underscore prefix"
            }
            WgslNameConflict::ReservedWord => "is a reserved WGSL keyword",
        }
    }
}

/// Classifies whether `name` collides with a WGSL reserved form.
///
/// A GPU function name is emitted verbatim as its WGSL entry-point/helper name,
/// so a name WGSL reserves would fail late at shader-module compilation with a
/// generic backend error. Returning the specific conflict lets the type checker
/// reject the name up front with a source-cited diagnostic and a rename hint.
/// Auto-generated kernel names (`miri_gpu_for_*`) never take these forms, so
/// this only fires on user-chosen `gpu fn` names.
pub fn wgsl_name_conflict(name: &str) -> Option<WgslNameConflict> {
    if name.starts_with("__") {
        return Some(WgslNameConflict::DoubleUnderscorePrefix);
    }
    if WGSL_RESERVED_WORDS.contains(&name) {
        return Some(WgslNameConflict::ReservedWord);
    }
    None
}

/// WGSL keywords and reserved words (WGSL specification, §keywords and
/// §reserved-words). Emitting a top-level function with any of these names
/// produces an invalid shader module. Keywords that are also Miri keywords
/// (`fn`, `let`, `var`, `struct`, …) can never reach here — the parser rejects
/// them as function names first — but they are listed for completeness so this
/// slice is the single source of truth for the reserved set.
const WGSL_RESERVED_WORDS: &[&str] = &[
    "NULL",
    "Self",
    "abstract",
    "active",
    "alias",
    "alignas",
    "alignof",
    "as",
    "asm",
    "asm_fragment",
    "async",
    "attribute",
    "auto",
    "await",
    "become",
    "binding_array",
    "break",
    "case",
    "cast",
    "catch",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "coherent",
    "column_major",
    "common",
    "compile",
    "compile_fragment",
    "concept",
    "const",
    "const_assert",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "continuing",
    "crate",
    "debugger",
    "decltype",
    "default",
    "delete",
    "demote",
    "demote_to_helper",
    "diagnostic",
    "discard",
    "do",
    "dynamic_cast",
    "else",
    "enable",
    "enum",
    "explicit",
    "export",
    "extends",
    "extern",
    "external",
    "fallthrough",
    "false",
    "filter",
    "final",
    "finally",
    "fn",
    "for",
    "friend",
    "from",
    "fxgroup",
    "get",
    "goto",
    "groupshared",
    "highp",
    "if",
    "impl",
    "implements",
    "import",
    "inline",
    "instanceof",
    "interface",
    "layout",
    "let",
    "loop",
    "lowp",
    "macro",
    "macro_rules",
    "match",
    "mediump",
    "meta",
    "mod",
    "module",
    "move",
    "mut",
    "mutable",
    "namespace",
    "new",
    "nil",
    "noexcept",
    "noinline",
    "nointerpolation",
    "noperspective",
    "null",
    "nullptr",
    "of",
    "operator",
    "override",
    "package",
    "packoffset",
    "partition",
    "pass",
    "patch",
    "pixelfragment",
    "precise",
    "precision",
    "premerge",
    "priv",
    "protected",
    "pub",
    "public",
    "readonly",
    "ref",
    "regardless",
    "register",
    "reinterpret_cast",
    "requires",
    "require",
    "resource",
    "restrict",
    "return",
    "self",
    "set",
    "shared",
    "sizeof",
    "smooth",
    "snorm",
    "static",
    "static_assert",
    "static_cast",
    "std",
    "struct",
    "subroutine",
    "super",
    "switch",
    "target",
    "template",
    "this",
    "thread_local",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "typedef",
    "typeid",
    "typename",
    "typeof",
    "union",
    "unless",
    "unorm",
    "unsafe",
    "unsized",
    "use",
    "using",
    "var",
    "varying",
    "virtual",
    "volatile",
    "wgsl",
    "where",
    "while",
    "with",
    "writeonly",
    "yield",
];

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

impl GpuAtomicOp {
    /// Maps a compiler-recognized atomic builtin function name to its operation.
    /// Returns `None` for any name that is not an atomic builtin.
    pub fn from_builtin_name(name: &str) -> Option<Self> {
        match name {
            "atomic_add" => Some(GpuAtomicOp::Add),
            "atomic_sub" => Some(GpuAtomicOp::Sub),
            "atomic_and" => Some(GpuAtomicOp::And),
            "atomic_or" => Some(GpuAtomicOp::Or),
            "atomic_xor" => Some(GpuAtomicOp::Xor),
            "atomic_min" => Some(GpuAtomicOp::Min),
            "atomic_max" => Some(GpuAtomicOp::Max),
            "atomic_exchange" => Some(GpuAtomicOp::Exchange),
            "atomic_compare_exchange" => Some(GpuAtomicOp::CompareExchange),
            _ => None,
        }
    }
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
