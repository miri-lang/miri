// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::backend::BackendMetadata;
use crate::mir::block::BasicBlockData;
use crate::mir::place::Local;
use std::fmt;

/// The body of a function in MIR.
///
/// A `Body` represents the complete control flow graph (CFG) for a single function
/// after lowering from AST. It contains:
/// - A sequence of basic blocks forming the CFG
/// - Declarations for all local variables (including temporaries)
/// - Metadata about the function's execution context
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Basic blocks in the control flow graph.
    /// Block 0 is always the entry block.
    pub basic_blocks: Vec<BasicBlockData>,
    /// Declarations of all local variables.
    /// Local 0 is reserved for the return value.
    /// Locals 1..=arg_count are the function parameters.
    pub local_decls: Vec<LocalDecl>,
    /// The number of arguments the function takes.
    pub arg_count: usize,
    /// The span of the entire function body.
    pub span: Span,
    /// The execution model for this function (CPU, GPU kernel, etc.)
    pub execution_model: ExecutionModel,
    /// Backend-specific metadata. None for CPU functions.
    pub backend_metadata: Option<BackendMetadata>,
}

impl Body {
    pub fn new(arg_count: usize, span: Span, execution_model: ExecutionModel) -> Self {
        Self {
            basic_blocks: Vec::new(),
            local_decls: Vec::new(),
            arg_count,
            span,
            execution_model,
            backend_metadata: None,
        }
    }

    pub fn new_local(&mut self, decl: LocalDecl) -> Local {
        let local = Local(self.local_decls.len());
        self.local_decls.push(decl);
        local
    }

    /// Returns true if this function runs on a GPU.
    pub fn is_gpu(&self) -> bool {
        matches!(
            self.execution_model,
            ExecutionModel::GpuKernel | ExecutionModel::GpuDevice
        )
    }
}

/// Specifies the execution context for a function body.
///
/// This determines how the function will be compiled and what intrinsics
/// are available. Designed to support multiple backend targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExecutionModel {
    /// Standard CPU execution (default for most functions)
    #[default]
    Cpu,
    /// Async function (returns a future/promise)
    Async,
    /// GPU kernel / compute shader entry point.
    /// Can be launched from CPU code via `GpuLaunch` terminator.
    GpuKernel,
    /// GPU device function (callable from kernels, but not launchable)
    GpuDevice,
}

impl fmt::Display for ExecutionModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionModel::Cpu => write!(f, "cpu"),
            ExecutionModel::Async => write!(f, "async"),
            ExecutionModel::GpuKernel => write!(f, "gpu_kernel"),
            ExecutionModel::GpuDevice => write!(f, "gpu_device"),
        }
    }
}

/// Storage class for local variables.
///
/// Determines where a variable is allocated in memory.
/// Universal classes are unprefixed; backend-specific classes are prefixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StorageClass {
    // === Universal (all backends) ===
    /// Stack-allocated local variable (default for CPU functions)
    #[default]
    Stack,

    // === GPU-specific memory spaces ===
    /// GPU shared memory (per-workgroup, accessible by all threads in block)
    /// - CUDA: __shared__
    /// - Metal: threadgroup
    /// - SPIR-V: Workgroup storage class
    GpuShared,
    /// GPU global memory (device-wide, accessible by all threads)
    /// - CUDA: __device__
    /// - Metal: device
    /// - SPIR-V: StorageBuffer
    GpuGlobal,
    /// GPU constant memory (read-only, cached)
    /// - CUDA: __constant__
    /// - Metal: constant
    /// - SPIR-V: Uniform/UniformConstant
    GpuConstant,
    /// GPU private memory (per-thread scratch space)
    /// - CUDA: local
    /// - Metal: thread
    /// - SPIR-V: Private
    GpuPrivate,

    // === Buffer bindings (GPU/accelerator APIs) ===
    /// Uniform buffer (read-only, for small frequently-accessed data)
    /// - Metal: constant buffer
    /// - SPIR-V/WebGPU: uniform
    UniformBuffer,
    /// Storage buffer (read-write, for large data)
    /// - Metal: buffer
    /// - SPIR-V/WebGPU: storage
    StorageBuffer,
}

impl fmt::Display for StorageClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageClass::Stack => write!(f, "stack"),
            StorageClass::GpuShared => write!(f, "gpu_shared"),
            StorageClass::GpuGlobal => write!(f, "gpu_global"),
            StorageClass::GpuConstant => write!(f, "gpu_constant"),
            StorageClass::GpuPrivate => write!(f, "gpu_private"),
            StorageClass::UniformBuffer => write!(f, "uniform"),
            StorageClass::StorageBuffer => write!(f, "storage"),
        }
    }
}

/// Declaration of a local variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalDecl {
    pub ty: Type,
    pub span: Span,
    pub name: Option<String>,
    pub is_user_variable: bool,
    pub storage_class: StorageClass,
}

impl LocalDecl {
    pub fn new(ty: Type, span: Span) -> Self {
        Self {
            ty,
            span,
            name: None,
            is_user_variable: false,
            storage_class: StorageClass::Stack,
        }
    }
}

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, decl) in self.local_decls.iter().enumerate() {
            let prefix = if decl.storage_class == StorageClass::Stack {
                String::new()
            } else {
                format!("{} ", decl.storage_class)
            };
            write!(f, "    {}let _{}: {};", prefix, i, decl.ty)?;
            if let Some(name) = &decl.name {
                write!(f, " // {}", name)?;
            }
            writeln!(f)?;
        }
        writeln!(f)?;

        for (i, block) in self.basic_blocks.iter().enumerate() {
            writeln!(f, "    bb{}: {{", i)?;
            for stmt in &block.statements {
                writeln!(f, "        {};", stmt)?;
            }
            if let Some(terminator) = &block.terminator {
                writeln!(f, "        {};", terminator)?;
            }
            writeln!(f, "    }}")?;
            writeln!(f)?;
        }
        Ok(())
    }
}
