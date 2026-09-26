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
    /// Names of custom types that never denote a heap object, and so are never
    /// reference counted.
    ///
    /// These are type aliases that resolve to an unmanaged type: `type Meters is
    /// int` reaches MIR as `Custom("Meters")`, but the value behind it is a bare
    /// integer with no allocation to release. Aliases to a managed type (`type ID
    /// is String`) are absent from this set and stay managed.
    ///
    /// Auto-copy structs and enums are deliberately NOT listed here. They are
    /// heap-allocated like any other aggregate, so they need releasing; their
    /// bitwise-copy semantics are a question about assignment, decided separately
    /// at lowering.
    pub unmanaged_type_names: HashSet<String>,
    /// Maps struct/class type names to their ordered field types (in layout order).
    /// Used by Perceus to resolve `Field(i)` place projections and determine
    /// whether the projected field is a managed type.
    pub field_types: HashMap<String, Vec<Type>>,
    /// Maps each generic class or struct to the names of its type parameters,
    /// in declaration order.
    /// Used by Perceus to read a `field_types` entry declared at a parameter
    /// (`value T`) at the type arguments of the instance it is projected from.
    pub class_type_params: HashMap<String, Vec<String>>,
    /// For closure/lambda bodies: the list of locals that hold captured values.
    /// Entry `i` is loaded at function entry from where codegen's capture
    /// layout places it, after the function and destructor pointers.
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
    /// Which parameters (indexed 1..=arg_count) are `out` parameters.
    /// `out_params[i-1] == true` means parameter `_i` was declared `out`.
    /// Populated by MIR lowering from `Parameter::is_out`.
    /// Used by codegen to emit pointer ABI for scalar out params (copy-in/copy-out).
    pub out_params: Vec<bool>,
    /// Which parameters (indexed 1..=arg_count) the body actually writes.
    /// `param_written[i-1] == true` means parameter `_i` is written by this body.
    /// Distinct from `out_params`, which for a kernel also drives the WGSL storage
    /// qualifier and stays true for atomic buffers regardless of data flow; this
    /// carries the truthful flow to backends that need it.
    pub param_written: Vec<bool>,
    /// Names of custom types that define a `fn drop(self)` method (resource types).
    /// Populated by MIR lowering from the type checker's struct/class definitions.
    /// Used by RC elision to avoid removing DecRef operations that trigger destructors.
    pub has_drop_types: HashSet<String>,
    /// GPU kernel workgroup sizes recorded during a launch in another function.
    /// Maps kernel name to [block_x, block_y, block_z].
    /// Collected when a `kernel(args).launch(grid, block)` call is lowered in a caller.
    /// Applied as metadata to the GPU kernel body's `BackendMetadata::Gpu.workgroup_size`
    /// during the post-lowering pipeline pass `stamp_kernel_workgroups`.
    pub kernel_workgroups: Vec<(String, [u32; 3])>,
    /// Launch grids written as a literal `Dim3` at a `kernel(args).launch(grid,
    /// block)` in this body, keyed by kernel name, as [grid_x, grid_y, grid_z]
    /// workgroup counts. A web bundle dispatches the grid it records, so it
    /// reads this; a grid computed at run time has no entry.
    pub kernel_grids: Vec<(String, [u32; 3])>,
    /// Every generic function instantiation this body calls, in call order.
    /// Recorded when the call is lowered, with the body's own instantiation
    /// substitution already applied, so the pipeline can lower each callee
    /// without recovering its type arguments from the mangled symbol.
    pub generic_function_calls: Vec<GenericFunctionCall>,
    /// Every call this body retargets to a residency-specialized body, in call
    /// order. Recorded when the call is lowered, so the pipeline lowers each
    /// specialization from the function it specializes without recovering that
    /// function from the specialized symbol.
    pub residency_function_calls: Vec<ResidencyFunctionCall>,
    /// Every generic class instantiation this body names only through its own
    /// instantiation substitution, in the order it was met.
    ///
    /// The type checker records the instantiations a program writes down, once,
    /// against the parameters of the body they appear in: `Box<T>` inside
    /// `via_box<T>` is `Box` at a placeholder. Which concrete `Box` that is exists
    /// only once `via_box` is lowered for a type, so the lowering records it and
    /// the pipeline adds it to the registry that decides which per-instantiation
    /// method bodies and drop functions get emitted.
    pub generic_class_instantiations: Vec<GenericClassInstantiation>,
    /// Boolean locals the readback pass keeps, each paired with the device
    /// handle it watches. A flag is set while its handle's device buffer may
    /// hold results the host array lacks, and cleared when a readback, an
    /// upload or a fresh activation brings the two back into agreement. A host
    /// read reached along paths that disagree about the buffer tests the flag
    /// rather than reading back every time, and the verifier takes a flag found
    /// clear as a fence.
    pub device_stale_flags: HashMap<Local, DeviceHandleId>,
}

/// One generic class at concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericClassInstantiation {
    /// The generic class's declared name.
    pub class: String,
    /// The class's type arguments, in declaration order.
    pub type_args: Vec<Type>,
}

/// One call to a generic function at concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericFunctionCall {
    /// The mangled symbol the call targets, e.g. `smaller__String`.
    pub symbol: String,
    /// The generic function's declared name.
    pub function: String,
    /// Each of the callee's generic parameters, paired with the type it is
    /// instantiated at.
    pub type_args: Vec<(String, Type)>,
}

/// One call to a function specialized for the gpu-resident buffers it passes.
#[derive(Debug, Clone, PartialEq)]
pub struct ResidencyFunctionCall {
    /// The residency-specialized symbol the call targets.
    pub symbol: String,
    /// The declared name of the function the specialization is lowered from.
    pub function: String,
    /// The device handle each positional argument carries, `None` for an
    /// argument that is not a gpu-resident buffer.
    pub arg_handles: Vec<Option<DeviceHandleId>>,
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
            unmanaged_type_names: HashSet::new(),
            field_types: HashMap::new(),
            class_type_params: HashMap::new(),
            env_capture_locals: Vec::new(),
            type_params: HashSet::new(),
            closure_capture_types: HashMap::new(),
            out_params: Vec::new(),
            param_written: Vec::new(),
            has_drop_types: HashSet::new(),
            kernel_workgroups: Vec::new(),
            kernel_grids: Vec::new(),
            generic_function_calls: Vec::new(),
            residency_function_calls: Vec::new(),
            generic_class_instantiations: Vec::new(),
            device_stale_flags: HashMap::new(),
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

    /// Every local the body stores into: the destination of an assignment or
    /// of a call.
    pub fn written_locals(&self) -> HashSet<Local> {
        use crate::mir::{StatementKind, TerminatorKind};

        let mut written = HashSet::new();
        for block in &self.basic_blocks {
            for stmt in &block.statements {
                if let StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) =
                    &stmt.kind
                {
                    written.insert(place.local);
                }
            }
            if let Some(TerminatorKind::Call { destination, .. }) =
                block.terminator.as_ref().map(|term| &term.kind)
            {
                written.insert(destination.local);
            }
        }
        written
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

/// Where a binding's value physically lives. Mirrors
/// [`crate::ast::statement::BindingResidency`] at the MIR level. The
/// default is [`BindingResidency::Host`]; lowering stamps `Gpu` on locals
/// introduced by `gpu let` / `gpu var`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BindingResidency {
    /// Standard host-side binding.
    #[default]
    Host,
    /// Device-resident binding.
    Gpu,
}

/// Stable identifier for the device buffer backing a `gpu`-resident binding.
///
/// Assigned at lowering to every local whose residency is
/// [`BindingResidency::Gpu`]. The runtime keys a persistent device buffer on
/// this id so kernel launches that capture the same binding share one buffer
/// across dispatches. Ids are allocated per compilation, so they are unique
/// within one build and identical across builds of the same source; `0` is
/// reserved by the runtime as the host-resident sentinel, so allocation starts
/// at `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceHandleId(pub u64);

impl fmt::Display for DeviceHandleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A value the host supplies to a GPU kernel from the launch itself rather
/// than from a captured program value.
///
/// Lowering stamps this on the kernel parameters it injects for a `forall`
/// (the per-axis loop bound and runtime range start) and for a `gpu frame`
/// (the element-count bound). Each binds as its own `i32` uniform, filled by
/// the launch, so a negative range start keeps its sign; every other uniform
/// parameter is a captured scalar, pooled into the kernel's scalar-capture
/// uniform. Backends classify a parameter by this marker, never by its name,
/// so a capture may be called anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchUniform {
    /// The exclusive end of an iteration axis.
    LoopBound,
    /// The runtime start of an iteration axis.
    RangeStart,
}

/// Declaration of a local variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalDecl {
    pub ty: Type,
    pub span: Span,
    pub name: Option<Rc<str>>,
    /// Where a declared binding's name is written in the source. Unlike
    /// `name`, it survives a release build, so it is what ties a local back to
    /// its declaration. Empty for temporaries and parameters.
    pub name_span: Span,
    pub is_user_variable: bool,
    pub storage_class: StorageClass,
    /// Where the binding's value lives (host / device). Orthogonal to
    /// [`StorageClass`], which classifies the memory address space inside
    /// a single residency.
    pub residency: BindingResidency,
    /// Persistent device buffer id for a `gpu`-resident binding. `None` for
    /// host-resident locals; `Some` is assigned at lowering whenever
    /// `residency` is [`BindingResidency::Gpu`].
    pub device_handle: Option<DeviceHandleId>,
    /// True when this local carries a `device_handle` it does not own — a
    /// residency-specialized parameter borrows the caller's persistent buffer.
    /// A borrowed handle is used to launch on the buffer but must never release
    /// it at scope exit; the owning binding in the caller frees it.
    pub device_handle_borrowed: bool,
    /// Set on a GPU kernel parameter the launch fills in; `None` for every
    /// other local, including captured scalars. See [`LaunchUniform`].
    pub launch_uniform: Option<LaunchUniform>,
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
            name_span: Span::default(),
            is_user_variable: false,
            storage_class: StorageClass::Stack,
            residency: BindingResidency::Host,
            device_handle: None,
            device_handle_borrowed: false,
            launch_uniform: None,
            mir_ty,
        }
    }
}

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, decl) in self.local_decls.iter().enumerate() {
            let storage_prefix = if decl.storage_class == StorageClass::Stack {
                String::new()
            } else {
                format!("{} ", decl.storage_class)
            };
            let residency_prefix = match decl.residency {
                BindingResidency::Host => "",
                BindingResidency::Gpu => "gpu ",
            };
            write!(
                f,
                "    {}{}let _{}: {};",
                residency_prefix, storage_prefix, i, decl.ty
            )?;
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
