// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::backend::BackendMetadata;
use crate::mir::block::BasicBlockData;
use crate::mir::place::Local;
use crate::mir::types::MirType;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

/// Maximum byte size for a type to qualify as auto-copy.
/// Types with all primitive/auto-copy fields and total size <= this are auto-copy.
pub const AUTO_COPY_MAX_SIZE: usize = 128;

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
    /// Names of custom types that have auto-copy semantics (all fields are
    /// primitive or other auto-copy types, and total size <= `AUTO_COPY_MAX_SIZE`).
    /// These types use bitwise copy on assignment and do not need RC.
    pub auto_copy_types: HashSet<String>,
    /// Maps struct/class type names to their ordered field types (in layout order).
    /// Used by Perceus to resolve `Field(i)` place projections and determine
    /// whether the projected field is a managed type.
    pub field_types: HashMap<String, Vec<Type>>,
    /// For closure/lambda bodies: the list of locals that hold captured values.
    /// Entry `i` is loaded from `env_ptr + (i+2) * ptr_size` at function entry.
    /// (slot 0 = fn_ptr, slot 1 = destructor_ptr, slots 2+ = captures)
    /// Empty for non-closure functions.
    pub env_capture_locals: Vec<Local>,
    /// Names of generic type parameters in scope for this function body.
    /// Used by `is_managed_type` to distinguish unresolved generic placeholders
    /// (which are never managed) from concrete user-defined types.
    /// Populated from the function's explicit generics and from `TypeKind::Generic`
    /// names found in parameter/return types (captures class-level generics too).
    pub type_params: HashSet<String>,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Populated by `lower_lambda_expr` after capture pruning.
    /// Used by Perceus to emit per-capture DecRef at StorageDead, and by codegen
    /// to resolve `Field(i)` projections on closure locals.
    /// Only present when the closure has at least one capture.
    pub closure_capture_types: HashMap<Local, Vec<Type>>,
}

impl Body {
    pub fn new(arg_count: usize, span: Span, execution_model: ExecutionModel) -> Self {
        Self {
            // Pre-allocate with reasonable defaults to reduce re-allocations
            // Basic blocks: entry + return + some branches
            basic_blocks: Vec::with_capacity(16),
            // Locals: args + return + some temporaries
            local_decls: Vec::with_capacity(arg_count + 16),
            arg_count,
            span,
            execution_model,
            backend_metadata: None,
            auto_copy_types: HashSet::new(),
            field_types: HashMap::new(),
            env_capture_locals: Vec::new(),
            type_params: HashSet::new(),
            closure_capture_types: HashMap::new(),
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

    /// Validate the consistency of the MIR body.
    /// Checks:
    /// 1. All blocks have a terminator.
    /// 2. All jump targets are valid block indices.
    pub fn validate(&self) -> Result<(), String> {
        for (i, block) in self.basic_blocks.iter().enumerate() {
            // 1. Check terminator
            if block.terminator.is_none() {
                return Err(format!("Basic block {} has no terminator", i));
            }

            // 2. Check targets
            if let Some(term) = &block.terminator {
                for target in term.successors() {
                    if target.0 >= self.basic_blocks.len() {
                        return Err(format!(
                            "Basic block {} jumps to invalid target bb{}",
                            i, target.0
                        ));
                    }
                }
            }
        }

        // 3. Check reachability (optional check, for now just ensure internal consistency)
        // We do not fail validation if blocks are unreachable, as that is valid MIR (dead code).
        // Use find_unreachable_blocks() if you need to detect them.
        Ok(())
    }

    /// Identify unreachable blocks in the CFG.
    /// Returns a list of block indices that cannot be reached from the entry block (bb0).
    pub fn find_unreachable_blocks(&self) -> Vec<usize> {
        if self.basic_blocks.is_empty() {
            return Vec::new();
        }

        let mut reachable = HashSet::new();
        let mut worklist = vec![0];
        reachable.insert(0);

        while let Some(idx) = worklist.pop() {
            if let Some(term) = &self.basic_blocks[idx].terminator {
                for target in term.successors() {
                    // target.0 is usize
                    if target.0 < self.basic_blocks.len() && reachable.insert(target.0) {
                        worklist.push(target.0);
                    }
                }
            }
        }

        (0..self.basic_blocks.len())
            .filter(|i| !reachable.contains(i))
            .collect()
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
    pub name: Option<Rc<str>>,
    pub is_user_variable: bool,
    pub storage_class: StorageClass,
    /// Resolved MIR-level type, free of AST expression nodes.
    ///
    /// Derived from `ty` at construction time via [`MirType::from_type_kind`].
    /// Used by analysis passes (e.g. Perceus) to traverse collection element
    /// types without pattern-matching on [`ExpressionKind`] nodes.
    ///
    /// [`ExpressionKind`]: crate::ast::expression::ExpressionKind
    pub mir_ty: MirType,
}

impl LocalDecl {
    pub fn new(ty: Type, span: Span) -> Self {
        let mir_ty = MirType::from_type_kind(&ty.kind);
        Self {
            ty,
            span,
            name: None,
            is_user_variable: false,
            storage_class: StorageClass::Stack,
            mir_ty,
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
