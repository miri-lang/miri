// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Cranelift code generation backend.
//!
//! This module provides the Cranelift-based code generator for CPU targets.
//! Cranelift is a fast code generator suitable for both JIT and AOT compilation.

pub(crate) mod closure;
mod closure_trampolines;
pub mod collection_header;
mod element_method_thunks;
mod gpu_launch;
mod inline_optionals;
mod inliner;
pub mod layout;
mod predicates;
mod rc;
mod source_points;
pub mod stable_headers;
pub mod structural_elements;
pub mod translate_rvalue;
pub mod translate_statement;
pub mod translator;
mod types;
pub mod value_abi;
pub(crate) mod value_identity_thunks;
pub mod value_locals;
mod vtable;

use crate::ast::types::{BuiltinCollectionKind, TypeKind, STRING_TYPE_NAME, TUPLE_TYPE_NAME};
use crate::codegen::backend::{
    ArtifactFormat, Backend, CodeRange, CompiledArtifact, FunctionCodeMap,
};
use crate::codegen::cranelift::element_method_thunks::ElementMethod;
use crate::error::CodegenError;
use crate::mir::option_layout::{LayoutScope, OptionalLayers};
use crate::mir::symbol::{StringLiteralPart, Symbol};
use crate::mir::type_facts::TypeDefinition;
use crate::mir::type_facts::TypeFacts;
use crate::mir::Body;
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::Signature;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use source_points::SourcePoints;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use target_lexicon::{DeploymentTarget, OperatingSystem, Triple};

pub use translator::FunctionTranslator;
use translator::{Emission, ProfileTable, RuntimeChecks};
pub use types::translate_type;

/// The symbol a profile build defines its function table under.
pub const PROFILE_FUNCTIONS_SYMBOL: &str = "miri_profile_functions";

/// Describes a runtime function to be declared as an external import.
///
/// These correspond to `#[no_mangle] extern "C"` functions in the runtime
/// library (e.g. `miri-runtime-core`) that are resolved at link time.
#[derive(Debug, Clone)]
pub struct RuntimeImport {
    /// Symbol name (e.g. `miri_rt_string_new`).
    pub name: String,
    /// Cranelift types for each parameter.
    pub param_types: Vec<cranelift_codegen::ir::Type>,
    /// Cranelift return type, or `None` for void functions.
    pub return_type: Option<cranelift_codegen::ir::Type>,
}

/// Cranelift backend for native code generation.
///
/// This backend uses Cranelift to generate machine code for the host platform.
/// It supports AOT compilation to object files which can be linked into executables.
pub struct CraneliftBackend {
    /// The target ISA (instruction set architecture).
    isa: Arc<dyn TargetIsa>,
    /// The program's settled type facts: type definitions, recorded
    /// generic-class instantiations and vtable fills.
    facts: TypeFacts,
    /// Runtime function imports to declare as external symbols.
    runtime_imports: Vec<RuntimeImport>,
    /// The bodies the GPU kernels' WGSL modules declare as device helpers.
    device_helpers: HashSet<Symbol>,
}

impl fmt::Debug for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CraneliftBackend")
            .field("target", &self.isa.triple().to_string())
            .finish()
    }
}

/// Cranelift backend compilation options.
#[derive(Debug, Clone)]
pub struct CraneliftOptions {
    /// Optimization level: "none", "speed", or "speed_and_size".
    pub opt_level: OptLevel,
    /// Whether to generate position-independent code.
    pub pic: bool,
    /// Whether to return the CLIF of every compiled function, as Cranelift
    /// left it after optimizing, in the artifact's `ir_listing`.
    pub ir_listing: bool,
    /// Whether compiled code keeps the runtime's checking hooks — the heap
    /// guard, the leak check and the allocation and reference-count counters —
    /// each behind a test of the runtime's tracking state. A release build
    /// leaves them out.
    pub runtime_checks: bool,
    /// Whether every function sets up a frame-pointer record, leaf functions
    /// included, so a sampling profiler walking frame pointers sees each
    /// caller. Without it AArch64 leaves the frame out of a leaf that makes
    /// no calls and uses no stack, and the leaf's caller drops out of the
    /// recorded stack.
    pub preserve_frame_pointers: bool,
    /// Whether to return, in the artifact's `code_map`, the source offset
    /// each run of every user function's code was generated from. Recording
    /// the offsets leaves the machine code unchanged.
    pub code_map: bool,
    /// Whether to define `miri_profile_functions`, the address and code size
    /// of every user function in the order the code map lists them, and have
    /// the entry point hand it to the runtime's sampler. Only a profile build
    /// asks for it.
    pub profile_table: bool,
    /// Whether small, loop-free user functions are inlined into their
    /// callers. An optimizing build asks for it.
    pub inline: bool,
}

/// What `compile` returns about each user function beside its code, as the
/// options ask for.
///
/// Each entry carries the position of its function among the user functions,
/// so the reports list the functions in that order whatever order they are
/// defined in.
struct FunctionReports {
    /// Each function's CLIF after Cranelift optimized it.
    ir_listing: Option<Vec<(usize, String)>>,
    /// Where each function's code came from.
    code_map: Option<Vec<(usize, FunctionCodeMap)>>,
    /// Each function's id and code size, for the profile function table.
    profile_functions: Option<Vec<(usize, ProfiledFunction)>>,
}

/// A user function's id and code size, as the profile function table lists
/// it.
type ProfiledFunction = (cranelift_module::FuncId, u32);

/// The reports on every user function, each listing the functions in the
/// order they were handed to the backend.
struct FinishedReports {
    ir_listing: Option<String>,
    code_map: Vec<FunctionCodeMap>,
    profile_functions: Option<Vec<ProfiledFunction>>,
}

impl FunctionReports {
    /// The reports `options` ask for, each empty.
    fn asked_by(options: &CraneliftOptions) -> Self {
        Self {
            ir_listing: options.ir_listing.then(Vec::new),
            code_map: options.code_map.then(Vec::new),
            profile_functions: options.profile_table.then(Vec::new),
        }
    }

    /// Every report, its entries put in the functions' order.
    fn finish(self) -> FinishedReports {
        FinishedReports {
            ir_listing: self.ir_listing.map(|listing| in_order(listing).concat()),
            code_map: self.code_map.map(in_order).unwrap_or_default(),
            profile_functions: self.profile_functions.map(in_order),
        }
    }
}

/// `entries` sorted by the position each carries, without the positions.
fn in_order<T>(mut entries: Vec<(usize, T)>) -> Vec<T> {
    entries.sort_by_key(|&(position, _)| position);
    entries.into_iter().map(|(_, entry)| entry).collect()
}

/// What translating a user function reads and adds to beside its body: the
/// module it declares into, the target, the string literal pool and the
/// GPU kernels a launch refers to.
struct TranslationEnv<'e> {
    module: &'e mut ObjectModule,
    isa: &'e Arc<dyn TargetIsa>,
    string_literals: &'e mut BTreeMap<String, usize>,
    kernel_registry: &'e HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
}

/// One user function handed to `translate_function`, and how its code is
/// emitted; whether it is the entry point is settled from its name.
struct FunctionUnit<'b> {
    name: &'b str,
    body: &'b Body,
    emission: Emission,
}

/// How `options` ask every user function's code to be emitted, before
/// the entry point is told apart from the rest.
fn function_emission(options: &CraneliftOptions, profile_table: Option<ProfileTable>) -> Emission {
    Emission {
        runtime_checks: if options.runtime_checks {
            RuntimeChecks::Observable
        } else {
            RuntimeChecks::CompiledOut
        },
        is_entry: false,
        source_locations: options.code_map,
        profile_table,
    }
}

/// The bodies of `bodies` that run on the CPU, in order.
fn cpu_bodies<'b>(bodies: &[(&'b str, &'b Body)]) -> Vec<(&'b str, &'b Body)> {
    bodies
        .iter()
        .copied()
        .filter(|(_, b)| {
            !matches!(
                b.execution_model,
                crate::mir::ExecutionModel::GpuKernel | crate::mir::ExecutionModel::GpuDevice
            )
        })
        .collect()
}

/// The namespace `cranelift-module` names a declared function in.
const FUNCTION_NAMESPACE: u32 = 0;

/// The namespace `cranelift-module` names a declared data object in.
const DATA_NAMESPACE: u32 = 1;

/// The user name `name` refers to in `func`'s table of external names.
fn user_name<'f>(
    func: &'f cranelift_codegen::ir::Function,
    name: &cranelift_codegen::ir::ExternalName,
) -> Option<&'f cranelift_codegen::ir::UserExternalName> {
    let cranelift_codegen::ir::ExternalName::User(user) = name else {
        return None;
    };
    func.params.user_named_funcs().get(*user)
}

/// The module function `name` refers to in `func`, `None` for a data
/// object or a name the module did not declare.
fn user_function_id(
    func: &cranelift_codegen::ir::Function,
    name: &cranelift_codegen::ir::ExternalName,
) -> Option<cranelift_module::FuncId> {
    user_name(func, name)
        .filter(|user| user.namespace == FUNCTION_NAMESPACE)
        .map(|user| cranelift_module::FuncId::from_u32(user.index))
}

/// One user function translated to CLIF and not yet defined.
struct TranslatedFunction<'b> {
    name: &'b str,
    id: cranelift_module::FuncId,
    function: cranelift_codegen::ir::Function,
    /// The places the function's instructions' locations index.
    source_points: SourcePoints,
    /// Whether the function is the program's entry point.
    is_entry: bool,
}

/// Optimization levels for Cranelift.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimizations.
    #[default]
    None,
    /// Optimize for speed.
    Speed,
    /// Optimize for both speed and size.
    SpeedAndSize,
}

impl OptLevel {
    /// The Cranelift tier a numeric optimization level selects: 0 does not
    /// optimize, and 1 through 3 all optimize for speed. Cranelift has a
    /// single speed tier, so the numbers above 1 are kept for the command
    /// line's sake and pick the same code. `SpeedAndSize` trades speed for
    /// size, which no numeric level asks for.
    pub fn for_level(level: u8) -> Self {
        if level == 0 {
            Self::None
        } else {
            Self::Speed
        }
    }

    /// The value Cranelift's `opt_level` setting takes for this tier.
    fn setting_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Speed => "speed",
            Self::SpeedAndSize => "speed_and_size",
        }
    }
}

impl CraneliftOptions {
    /// Whether these options select the same Cranelift flags as the defaults,
    /// which the backend's cached target ISA was built with.
    fn uses_default_isa(&self) -> bool {
        let defaults = Self::default();
        self.opt_level == defaults.opt_level
            && self.pic == defaults.pic
            && self.preserve_frame_pointers == defaults.preserve_frame_pointers
    }
}

impl Default for CraneliftOptions {
    fn default() -> Self {
        Self {
            opt_level: OptLevel::None,
            pic: true,
            ir_listing: false,
            runtime_checks: true,
            preserve_frame_pointers: false,
            code_map: false,
            profile_table: false,
            inline: false,
        }
    }
}

impl CraneliftBackend {
    /// Create a new Cranelift backend for the host platform.
    pub fn new() -> Result<Self, CodegenError> {
        let mut target = Triple::host();
        // If we are on macOS (Darwin), we need to specify a version so that Cranelift
        // emits the LC_BUILD_VERSION load command. Without this, the linker will warn.
        match target.operating_system {
            OperatingSystem::Darwin(_) | OperatingSystem::MacOSX(_) => {
                target.operating_system = OperatingSystem::MacOSX(Some(DeploymentTarget {
                    major: 12,
                    minor: 0,
                    patch: 0,
                }));
            }
            _ => {}
        }
        Self::for_target(target)
    }

    /// Create a new Cranelift backend for a specific target.
    pub fn for_target(target: Triple) -> Result<Self, CodegenError> {
        let isa = build_isa(target, &CraneliftOptions::default())?;

        Ok(Self {
            isa,
            facts: TypeFacts::default(),
            runtime_imports: Vec::new(),
            device_helpers: HashSet::new(),
        })
    }

    /// Get the target triple this backend is configured for.
    pub fn target(&self) -> &Triple {
        self.isa.triple()
    }

    /// Get the pointer type for the target ISA.
    pub fn pointer_type(&self) -> cranelift_codegen::ir::Type {
        self.isa.pointer_type()
    }

    /// Set the settled type facts the backend reads while emitting code: type
    /// definitions for layout, recorded generic-class instantiations for
    /// per-instantiation thunks, and the slots each vtable fills. A vtable no
    /// fill names is defined with every slot null.
    pub fn set_type_facts(&mut self, facts: TypeFacts) {
        self.facts = facts;
    }

    /// Set the bodies the GPU kernels' WGSL modules declare as device helpers.
    pub fn set_device_helpers(&mut self, device_helpers: HashSet<Symbol>) {
        self.device_helpers = device_helpers;
    }

    /// Set runtime function imports that should be declared as external symbols.
    pub fn set_runtime_imports(&mut self, imports: Vec<RuntimeImport>) {
        self.runtime_imports = imports;
    }

    /// Declare all runtime function imports in the given object module.
    ///
    /// Each import is registered with `Linkage::Import`, meaning the symbol
    /// must be resolved at link time (typically from a static runtime library).
    fn declare_runtime_imports(&self, module: &mut ObjectModule) -> Result<(), CodegenError> {
        let call_conv = self.isa.default_call_conv();

        for import in &self.runtime_imports {
            let mut sig = Signature::new(call_conv);

            for &param_ty in &import.param_types {
                sig.params.push(AbiParam::new(param_ty));
            }

            if let Some(ret_ty) = import.return_type {
                sig.returns.push(AbiParam::new(ret_ty));
            }

            module
                .declare_function(&import.name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::declare_function(&import.name, e.to_string()))?;
        }

        Ok(())
    }
}

// NOTE: No Default impl — CraneliftBackend::new() can fail (target ISA lookup).
// Callers must use CraneliftBackend::new() and handle the Result explicitly.

impl Backend for CraneliftBackend {
    type Error = CodegenError;
    type Options = CraneliftOptions;

    /// Compile the provided MIR bodies into an artifact.
    ///
    /// Pipeline: resolve ISA → create object module → declare runtime imports →
    /// generate drop thunks / closure dtors → predeclare user fns → compile each
    /// body → generate vtables → emit string literals → finalize object.
    fn compile(
        &self,
        bodies: &[(&str, &Body)],
        options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        let isa = self.resolve_isa(options)?;
        let (mut module, mut ctx) = Self::create_object_module(&isa)?;

        self.declare_runtime_imports(&mut module)?;
        self.generate_type_drop_functions(&mut module, &mut ctx, &isa)?;
        self.generate_structural_element_decref_functions(&mut module, &mut ctx, &isa, bodies)?;
        self.generate_element_method_thunks(&mut module, &mut ctx, &isa)?;
        self.generate_lambda_destructors(&mut module, &mut ctx, &isa, bodies)?;
        let kernel_registry = crate::codegen::cranelift::gpu_launch::build_kernel_registry(
            &mut module,
            bodies,
            &self.device_helpers,
        )?;
        let cpu_bodies = cpu_bodies(bodies);
        closure_trampolines::generate_closure_trampolines(
            &mut module,
            &mut ctx,
            &isa,
            &cpu_bodies,
        )?;
        self.predeclare_user_functions(&mut module, &isa, &cpu_bodies)?;

        let mut string_literals = BTreeMap::new();
        let profile_table = options
            .profile_table
            .then(|| Self::declare_profile_table(&mut module, cpu_bodies.len()))
            .transpose()?;
        let mut env = TranslationEnv {
            module: &mut module,
            isa: &isa,
            string_literals: &mut string_literals,
            kernel_registry: &kernel_registry,
        };
        let emission = function_emission(options, profile_table);
        let mut reports = FunctionReports::asked_by(options);
        if options.inline {
            self.compile_inlining(&mut env, &mut ctx, &cpu_bodies, emission, &mut reports)?;
        } else {
            self.compile_each(&mut env, &mut ctx, &cpu_bodies, emission, &mut reports)?;
        }
        let reports = reports.finish();

        self.define_vtables(&mut module, &isa, &cpu_bodies)?;
        Self::define_string_literals(&mut module, &isa, string_literals)?;
        if let (Some(table), Some(functions)) = (profile_table, &reports.profile_functions) {
            Self::define_profile_table(&mut module, &isa, table, functions)?;
        }
        let object = self.finalize_object(module)?;

        Ok(CompiledArtifact {
            bytes: object,
            format: ArtifactFormat::ObjectFile,
            ir_listing: reports.ir_listing,
            code_map: reports.code_map,
        })
    }

    fn name(&self) -> &'static str {
        "cranelift"
    }
}

/// Let a 128-bit scalar appear in a function signature.
///
/// The x86-64 ABIs each describe a different way of handing over a 128-bit
/// integer — a vector register, a pair of general registers, or a pointer to a
/// copy — so the backend refuses to pick one on its own and aborts the process
/// the moment such a parameter or return value reaches signature lowering.
/// Asking for the LLVM lowering settles it: the value travels as two 64-bit
/// halves placed exactly where two consecutive 64-bit arguments would go, and
/// both halves move to the stack together when either one has to. That is what
/// a C compiler does with `__int128` on the same platform, so a `i128` or
/// `u128` crossing into the runtime keeps matching the Rust side that receives
/// it. AArch64 lowers the same signature without being asked and is unaffected.
fn enable_wide_scalar_abi(builder: &mut settings::Builder) -> Result<(), String> {
    builder
        .set("enable_llvm_abi_extensions", "true")
        .map_err(|e| e.to_string())
}

/// The target ISA `options` ask Cranelift for on `target`.
fn build_isa(
    target: Triple,
    options: &CraneliftOptions,
) -> Result<Arc<dyn TargetIsa>, CodegenError> {
    let flags = isa_flags(options).map_err(CodegenError::TargetIsa)?;
    cranelift_codegen::isa::lookup(target)
        .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
        .finish(flags)
        .map_err(|e| CodegenError::TargetIsa(e.to_string()))
}

/// The shared Cranelift flags `options` select.
fn isa_flags(options: &CraneliftOptions) -> Result<settings::Flags, String> {
    let mut builder = settings::builder();
    let requested = [
        ("opt_level", options.opt_level.setting_value()),
        ("is_pic", flag_value(options.pic)),
        (
            "preserve_frame_pointers",
            flag_value(options.preserve_frame_pointers),
        ),
    ];
    for (name, value) in requested {
        builder.set(name, value).map_err(|e| e.to_string())?;
    }
    enable_wide_scalar_abi(&mut builder)?;
    Ok(settings::Flags::new(builder))
}

fn flag_value(enabled: bool) -> &'static str {
    if enabled {
        "true"
    } else {
        "false"
    }
}

/// The environment variable that opts a failing build into a backend IR dump.
const DUMP_IR_ENV: &str = "MIRI_DUMP_BACKEND_IR";

/// What a failed `define_function` reports.
///
/// The backend's own IR is the first thing a compiler engineer wants here and
/// the last thing a caller of the published diagnostics envelope should
/// receive: it carries no source position, cannot be acted on, and is compiler
/// internals reaching a machine-readable contract. It travels only when
/// `MIRI_DUMP_BACKEND_IR` is set, which is how someone debugging the backend
/// asks for it.
fn define_failure_details(error: &impl fmt::Display, ctx: &Context) -> String {
    if std::env::var_os(DUMP_IR_ENV).is_some() {
        format!("{}\n\nCranelift IR:\n{}", error, ctx.func.display())
    } else {
        error.to_string()
    }
}

impl CraneliftBackend {
    /// Resolve the target ISA. Reuses the cached default ISA when the options
    /// ask for the same flags; otherwise builds one with the requested flags.
    fn resolve_isa(&self, options: &CraneliftOptions) -> Result<Arc<dyn TargetIsa>, CodegenError> {
        if options.uses_default_isa() {
            return Ok(self.isa.clone());
        }
        build_isa(self.isa.triple().clone(), options)
    }

    /// Create the `ObjectModule` + per-function `Context` used to build the
    /// final artifact.
    fn create_object_module(
        isa: &Arc<dyn TargetIsa>,
    ) -> Result<(ObjectModule, Context), CodegenError> {
        let object_builder = ObjectBuilder::new(
            isa.clone(),
            "miri_module",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CodegenError::Module(e.to_string()))?;
        Ok((ObjectModule::new(object_builder), Context::new()))
    }

    /// Generate `miri.$dtor.{lambda_name}` destructors for closure bodies that
    /// capture managed values. Must run before user fns so call sites can
    /// reference them via `Linkage::Import`.
    fn generate_lambda_destructors(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
        bodies: &[(&str, &Body)],
    ) -> Result<(), CodegenError> {
        for (name, body) in bodies.iter() {
            if body.env_capture_locals.is_empty() {
                continue;
            }
            let has_managed = body.env_capture_locals.iter().any(|&cap_local| {
                crate::mir::rc::is_slot_counted(
                    &body.local_decls[cap_local.0].ty.kind,
                    body.layout_scope(),
                )
            });
            if has_managed {
                FunctionTranslator::generate_closure_destructor(
                    module,
                    ctx,
                    isa,
                    name,
                    body,
                    &self.facts,
                )?;
            }
        }
        Ok(())
    }

    /// Pre-declare all user functions with MIR-derived signatures so call sites
    /// compiled before the callee resolve to the correct signature (avoids
    /// the DFG-inferred widened-type mismatch). Each is the signature its body
    /// is defined under.
    fn predeclare_user_functions(
        &self,
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        bodies: &[(&str, &Body)],
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        for (name, body) in bodies.iter() {
            let sig = value_abi::body_signature(body, call_conv, ptr_type);
            module
                .declare_function(name, Linkage::Export, &sig)
                .map_err(|e| CodegenError::declare_function(*name, e.to_string()))?;
        }
        Ok(())
    }

    /// Define collected string literals as immortal static data structures
    /// (`[RC][DataPtr][Len][Cap]`) referencing a sibling `*_bytes` data symbol.
    ///
    /// The iteration order of `string_literals` affects the order of static data
    /// symbols in the emitted object file. Using a `BTreeMap` ensures this order is
    /// deterministic across builds, producing byte-identical artifacts from identical source.
    fn define_string_literals(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        string_literals: BTreeMap<String, usize>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        for (literal, index) in string_literals {
            Self::define_one_string_literal(module, &literal, index, ptr_size)?;
        }
        Ok(())
    }

    /// Declare the profile function table, to be defined once every function
    /// listed in it has been compiled and its size is known.
    fn declare_profile_table(
        module: &mut ObjectModule,
        count: usize,
    ) -> Result<ProfileTable, CodegenError> {
        let data = module
            .declare_data(PROFILE_FUNCTIONS_SYMBOL, Linkage::Export, false, false)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        Ok(ProfileTable { data, count })
    }

    /// Define the profile function table: per function, in the order the
    /// backend was handed the functions, a
    /// pointer-sized relocation to its first instruction and its code size
    /// as a 32-bit word after it, each entry padded to two pointers — the
    /// `ProfileFunction` layout the runtime reads.
    fn define_profile_table(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        table: ProfileTable,
        functions: &[(cranelift_module::FuncId, u32)],
    ) -> Result<(), CodegenError> {
        let pointer = isa.pointer_type().bytes() as usize;
        let entry = 2 * pointer;
        let mut bytes = vec![0u8; entry * functions.len()];
        for (slot, (_, size)) in bytes.chunks_exact_mut(entry).zip(functions) {
            slot[pointer..pointer + 4].copy_from_slice(&size.to_ne_bytes());
        }
        let mut description = DataDescription::new();
        description.set_align(pointer as u64);
        description.define(bytes.into_boxed_slice());
        for (index, &(func_id, _)) in functions.iter().enumerate() {
            let function = module.declare_func_in_data(func_id, &mut description);
            let offset = u32::try_from(index * entry).map_err(|_| {
                CodegenError::Module("the profile function table is too large".to_string())
            })?;
            description.write_function_addr(offset, function);
        }
        module
            .define_data(table.data, &description)
            .map_err(|e| CodegenError::Module(e.to_string()))
    }

    /// Emit the byte data + MiriString struct for the `index`-th string literal.
    fn define_one_string_literal(
        module: &mut ObjectModule,
        literal: &str,
        index: usize,
        ptr_size: u32,
    ) -> Result<(), CodegenError> {
        let bytes_id = Self::define_string_bytes(module, literal, index)?;

        let struct_symbol = Symbol::string_literal(index, StringLiteralPart::Object).link_name();
        let struct_id = module
            .declare_data(&struct_symbol, Linkage::Export, false, false)
            .map_err(|e| CodegenError::Module(e.to_string()))?;

        let mut struct_ctx = DataDescription::new();
        struct_ctx.set_align(ptr_size as u64);
        struct_ctx.define(
            Self::build_miri_string_struct_bytes(
                literal.len() as u64,
                literal.is_ascii(),
                ptr_size,
            )
            .into_boxed_slice(),
        );

        // Relocation for the data pointer at offset ptr_size
        let bytes_ref = module.declare_data_in_data(bytes_id, &mut struct_ctx);
        struct_ctx.write_data_addr(ptr_size, bytes_ref, 0);

        module
            .define_data(struct_id, &struct_ctx)
            .map_err(|e| CodegenError::Module(e.to_string()))
    }

    /// Define the raw byte data of the `index`-th string literal; return the
    /// `DataId` of the byte array.
    fn define_string_bytes(
        module: &mut ObjectModule,
        literal: &str,
        index: usize,
    ) -> Result<cranelift_module::DataId, CodegenError> {
        let bytes_symbol = Symbol::string_literal(index, StringLiteralPart::Bytes).link_name();
        let bytes_id = module
            .declare_data(&bytes_symbol, Linkage::Export, false, false)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        let mut bytes_ctx = DataDescription::new();
        bytes_ctx.define(literal.as_bytes().to_vec().into_boxed_slice());
        module
            .define_data(bytes_id, &bytes_ctx)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        Ok(bytes_id)
    }

    /// Build the `MiriString` struct payload: `[RC | DataPtr | Len | Cap |
    /// Ascii]` with the high RC bit set so the runtime treats the literal as
    /// immortal, and the ASCII flag the runtime reads a character index by.
    /// The DataPtr slot (offset `ptr_size`) is left zero — the caller writes
    /// the relocation via `write_data_addr`.
    fn build_miri_string_struct_bytes(len: u64, ascii: bool, ptr_size: u32) -> Vec<u8> {
        let mut data = vec![0u8; 5 * ptr_size as usize];
        data[4 * ptr_size as usize] = u8::from(ascii);
        if ptr_size == 4 {
            data[0..4].copy_from_slice(&(1u32 << 31).to_ne_bytes());
            data[8..12].copy_from_slice(&(len as u32).to_ne_bytes());
            data[12..16].copy_from_slice(&(len as u32).to_ne_bytes());
        } else {
            data[0..8].copy_from_slice(&(1u64 << 63).to_ne_bytes());
            data[16..24].copy_from_slice(&len.to_ne_bytes());
            data[24..32].copy_from_slice(&len.to_ne_bytes());
        }
        data
    }

    /// Finish the object module and inject the macOS build-version load command
    /// when targeting Darwin (cranelift-object doesn't do this automatically).
    fn finalize_object(&self, module: ObjectModule) -> Result<Vec<u8>, CodegenError> {
        let mut product = module.finish();

        if matches!(
            self.target().operating_system,
            OperatingSystem::Darwin(_) | OperatingSystem::MacOSX(_)
        ) {
            // Platform 1 = macOS. Version 0x000C0000 = 12.0.0. SDK 0 = none.
            let mut info = cranelift_object::object::write::MachOBuildVersion::default();
            info.platform = 1;
            info.minos = 0x000C0000;
            info.sdk = 0;
            product.object.set_macho_build_version(info);
        }

        product
            .emit()
            .map_err(|e| CodegenError::Emit(e.to_string()))
    }

    /// Translate, define and drop each of `bodies` in turn, so no more than
    /// one function's IR is held at a time.
    fn compile_each(
        &self,
        env: &mut TranslationEnv<'_>,
        ctx: &mut Context,
        bodies: &[(&str, &Body)],
        emission: Emission,
        reports: &mut FunctionReports,
    ) -> Result<(), CodegenError> {
        for (position, &(name, body)) in bodies.iter().enumerate() {
            let unit = FunctionUnit {
                name,
                body,
                emission,
            };
            let translated = self.translate_function(env, unit)?;
            Self::define_translated(env.module, ctx, position, translated, reports)?;
        }
        Ok(())
    }

    /// Translate every one of `bodies`, inline small functions into their
    /// callers, and define each function once it has been inlined into.
    fn compile_inlining(
        &self,
        env: &mut TranslationEnv<'_>,
        ctx: &mut Context,
        bodies: &[(&str, &Body)],
        emission: Emission,
        reports: &mut FunctionReports,
    ) -> Result<(), CodegenError> {
        let translated = bodies
            .iter()
            .map(|&(name, body)| {
                let unit = FunctionUnit {
                    name,
                    body,
                    emission,
                };
                self.translate_function(env, unit)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let module = &mut *env.module;
        inliner::inline_and_define(translated, &**env.isa, |position, function| {
            Self::define_translated(module, ctx, position, function, reports)
        })
    }

    /// Translate a single MIR function body to Cranelift IR and declare it.
    fn translate_function<'b>(
        &self,
        env: &mut TranslationEnv<'_>,
        unit: FunctionUnit<'b>,
    ) -> Result<TranslatedFunction<'b>, CodegenError> {
        let FunctionUnit {
            name,
            body,
            emission,
        } = unit;
        let mut translator = FunctionTranslator::new(env.isa, body, &self.facts);
        // Every executable's entry point is linked under this one name, a
        // script's synthesized `main` included.
        let is_entry = name == Symbol::entry().link_name();
        let emission = Emission {
            is_entry,
            ..emission
        };
        translator
            .translate(
                body,
                env.module,
                env.string_literals,
                env.kernel_registry,
                emission,
            )
            .map_err(|e| CodegenError::translation(name, e.to_string()))?;
        let sig = translator.signature().clone();
        let (function, source_points) = translator.into_parts();
        let id = env
            .module
            .declare_function(name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;
        Ok(TranslatedFunction {
            name,
            id,
            function,
            source_points,
            is_entry,
        })
    }

    /// Define a translated function, the `position`-th user function, and
    /// report on it as `reports` ask.
    fn define_translated(
        module: &mut ObjectModule,
        ctx: &mut Context,
        position: usize,
        translated: TranslatedFunction<'_>,
        reports: &mut FunctionReports,
    ) -> Result<(), CodegenError> {
        let TranslatedFunction {
            name,
            id,
            function,
            source_points,
            is_entry: _,
        } = translated;
        ctx.func = function;
        crate::codegen::cranelift::define_function(module, id, ctx)
            .map_err(|e| CodegenError::define_function(name, define_failure_details(&e, ctx)))?;
        if let Some(code_map) = reports.code_map.as_mut() {
            code_map.push((
                position,
                Self::function_code_map(name, ctx, &source_points)?,
            ));
        }
        if let Some(functions) = reports.profile_functions.as_mut() {
            let size = ctx
                .compiled_code()
                .map_or(0, |code| code.code_info().total_size);
            functions.push((position, (id, size)));
        }
        if let Some(listing) = reports.ir_listing.as_mut() {
            listing.push((
                position,
                format!(
                    "; {name}\n{}{}\n",
                    ctx.func.display(),
                    Self::listing_references(&ctx.func, module.declarations())
                ),
            ));
        }
        ctx.clear();
        Ok(())
    }

    /// Where the code `ctx` holds for the function `name` came from: every
    /// run Cranelift recorded a location for, read off `source_points`, with
    /// the calls it was inlined through.
    fn function_code_map(
        name: &str,
        ctx: &Context,
        source_points: &SourcePoints,
    ) -> Result<FunctionCodeMap, CodegenError> {
        let compiled = ctx.compiled_code().ok_or_else(|| {
            CodegenError::define_function(name, "no compiled code to map to source")
        })?;
        let ranges = compiled
            .buffer
            .get_srclocs_sorted()
            .iter()
            .filter_map(|run| {
                let point = source_points.point_at(run.loc)?;
                Some(CodeRange {
                    start: run.start,
                    end: run.end,
                    source: point.source,
                    source_offset: point.offset,
                    inlined_at: source_points.call_chain(point),
                })
            })
            .collect();
        Ok(FunctionCodeMap {
            symbol: name.to_string(),
            code_size: compiled.code_info().total_size,
            ranges,
        })
    }

    /// Define every vtable the CPU bodies construct.
    fn define_vtables(
        &self,
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        cpu_bodies: &[(&str, &Body)],
    ) -> Result<(), CodegenError> {
        let vtables = crate::mir::dispatch::constructed_vtable_symbols(
            cpu_bodies.iter().map(|(_, body)| *body),
            self.facts.definitions(),
        );
        FunctionTranslator::generate_vtables(
            module,
            isa.pointer_type(),
            &self.facts,
            vtables.iter().map(|(symbol, drop)| {
                (
                    symbol.as_str(),
                    drop.as_str(),
                    self.facts.vtable_fills().slots(symbol),
                )
            }),
        )
    }

    /// One comment line per function and symbol `func` refers to, naming it: the
    /// CLIF text identifies an external function or global only by its index in
    /// the module, which a reader of the listing cannot resolve.
    fn listing_references(
        func: &cranelift_codegen::ir::Function,
        declarations: &cranelift_module::ModuleDeclarations,
    ) -> String {
        use cranelift_codegen::ir::{ExternalName, GlobalValueData};
        let declared_name = |external: &ExternalName| -> Option<String> {
            if let Some(id) = user_function_id(func, external) {
                let decl = declarations.get_function_decl(id);
                return Some(decl.linkage_name(id).into_owned());
            }
            let data = user_name(func, external).filter(|user| user.namespace == DATA_NAMESPACE)?;
            let id = cranelift_module::DataId::from_u32(data.index);
            Some(declarations.get_data_decl(id).linkage_name(id).into_owned())
        };
        let mut out = String::new();
        for (func_ref, data) in func.dfg.ext_funcs.iter() {
            if let Some(name) = declared_name(&data.name) {
                out.push_str(&format!("; {func_ref} = {name}\n"));
            }
        }
        for (global, data) in func.global_values.iter() {
            if let GlobalValueData::Symbol { name: external, .. } = data {
                if let Some(name) = declared_name(external) {
                    out.push_str(&format!("; {global} = {name}\n"));
                }
            }
        }
        out
    }

    /// Generates `miri.TypeName.$drop(ptr)` functions for every managed concrete type.
    ///
    /// A type is "managed" if it is a non-generic struct, class, or enum that has
    /// at least one field of a managed (heap-allocated) type. These drop functions
    /// form the foundation of the Perceus RC destructor pipeline:
    ///
    ///   RC reaches 0 → call `miri.TypeName.$drop(ptr)`
    ///     → (1) user-defined `fn drop(self)` hook (if the type defines one)
    ///     → (2) recursively DecRef all managed fields
    ///     → (3) free the RC allocation
    ///
    /// Types with no managed fields and no user drop hook skip the thunk; their
    /// drop path calls `libc::free` directly from `emit_type_drop`.
    fn generate_type_drop_functions(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
    ) -> Result<(), CodegenError> {
        // Collect all Struct/Class/Enum types and sort for deterministic output.
        // We include types without managed fields so that `miri.TypeName.$decref` can be
        // generated for them — it is needed as elem_drop_fn when such types are
        // stored in a List, Set, or Map. A generic struct, class or enum is
        // accepted: the bare thunk serves as the shared entry point and skips a
        // field still written at a parameter, while the per-instantiation thunks
        // below resolve each field against concrete arguments, so the field an
        // instance actually stores is the one released.
        //
        // Builtin collection class names (`List`, `Map`, `Set`, `Array`, `Tuple`)
        // and `String` are skipped: their drop / decref / clone paths route through
        // dedicated runtime helpers (`miri_rt_list_free`, …) inside `emit_type_drop`
        // before any `miri.TypeName.$drop` thunk is consulted, so emitting one would be
        // dead code that bloats the object file.
        let mut managed_names: Vec<&str> = self
            .facts
            .definitions()
            .iter()
            .filter_map(|(name, def)| {
                match def {
                    TypeDefinition::Struct(_)
                    | TypeDefinition::Class(_)
                    | TypeDefinition::Enum(_) => {}
                    TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_)
                    | TypeDefinition::Trait(_) => return None,
                }
                if BuiltinCollectionKind::from_name(name).is_some()
                    || name == STRING_TYPE_NAME
                    || name == TUPLE_TYPE_NAME
                {
                    return None;
                }
                Some(name.as_str())
            })
            .collect();
        managed_names.sort_unstable();

        // A trait has no fields and no drop thunk of its own, but a collection
        // of trait-typed elements releases each through `miri.Trait.$decref`,
        // which dispatches on the element's runtime class.
        let mut trait_names: Vec<&str> = self
            .facts
            .definitions()
            .iter()
            .filter(|(_, def)| matches!(def, TypeDefinition::Trait(_)))
            .map(|(name, _)| name.as_str())
            .collect();
        trait_names.sort_unstable();
        for trait_name in trait_names {
            FunctionTranslator::generate_decref_function(
                module,
                ctx,
                isa,
                trait_name,
                &[],
                rc::Release::Trait,
            )?;
        }

        for type_name in managed_names {
            // Bare `miri.TypeName.$drop` (+ its `miri.TypeName.$decref` wrapper). For a
            // generic class this thunk backs the collection-element decref path;
            // direct drops route through the per-instantiation thunks below.
            FunctionTranslator::generate_drop_function(
                module,
                ctx,
                isa,
                type_name,
                None,
                &self.facts,
            )?;
            FunctionTranslator::generate_clone_function(
                module,
                ctx,
                isa,
                type_name,
                None,
                &self.facts,
            )?;
            for args in self.monomorphized_instantiations(type_name) {
                FunctionTranslator::generate_clone_function(
                    module,
                    ctx,
                    isa,
                    type_name,
                    Some(args),
                    &self.facts,
                )?;
            }
            self.generate_instantiation_drop_functions(module, ctx, isa, type_name)?;
        }
        Ok(())
    }

    /// Generate `miri.TypeName.$compare` for every type whose values carry an
    /// order, and `miri.TypeName.$equals` for every type that defines its own
    /// equality, so a container of them can sort or match its elements.
    ///
    /// Kept apart from the drop-thunk pass: that one skips the built-in classes
    /// whose drop path routes through a runtime helper, and one of those —
    /// the string type — is the element type sorting by content matters most
    /// for. Names are sorted so the object file comes out the same on every
    /// build.
    ///
    /// A generic class also gets one thunk per recorded instantiation, so a
    /// `Box<String>` element is asked through the body compiled for `String`
    /// rather than by the shared one, which reads its own parameter.
    fn generate_element_method_thunks(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
    ) -> Result<(), CodegenError> {
        let mut names: Vec<&str> = self
            .facts
            .definitions()
            .keys()
            .map(String::as_str)
            .filter(|name| {
                // An array or list answers `equals` element by element; a set
                // or map answers neither question.
                !matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Map | BuiltinCollectionKind::Set)
                )
            })
            .collect();
        names.sort_unstable();
        for method in ElementMethod::ALL {
            for type_name in &names {
                FunctionTranslator::generate_element_method_thunk(
                    method,
                    module,
                    ctx,
                    isa,
                    type_name,
                    None,
                    &self.facts,
                )?;
                self.generate_instantiation_method_thunks(method, module, ctx, isa, type_name)?;
            }
        }
        Ok(())
    }

    /// Generate the `method` thunk for each recorded instantiation of a generic
    /// class that answers it, once per thunk symbol so the same symbol is never
    /// defined twice.
    ///
    /// A thunk calls a method body the pipeline lowered for that instantiation,
    /// so an instantiation whose arguments the pipeline does not monomorphize —
    /// `Box<T>` recorded inside `Box`'s own body — gets none: its thunk would
    /// name a symbol nothing defines.
    fn generate_instantiation_method_thunks(
        &self,
        method: ElementMethod,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
    ) -> Result<(), CodegenError> {
        for args in self.monomorphized_instantiations(type_name) {
            FunctionTranslator::generate_element_method_thunk(
                method,
                module,
                ctx,
                isa,
                type_name,
                Some(args),
                &self.facts,
            )?;
        }
        Ok(())
    }

    /// The recorded instantiations of the generic class `type_name` a
    /// per-instantiation body is lowered for, each once: those whose
    /// arguments are all monomorphizable. Empty for any other type.
    fn monomorphized_instantiations(&self, type_name: &str) -> Vec<&[crate::ast::types::Type]> {
        let Some(TypeDefinition::Class(class_def)) = self.facts.definitions().get(type_name) else {
            return Vec::new();
        };
        if class_def.generics.is_none() {
            return Vec::new();
        }
        let Some(tuples) = self.facts.generic_class_instantiations().get(type_name) else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        tuples
            .iter()
            .filter(|args| {
                args.iter().all(|arg| {
                    crate::mir::instantiation::is_monomorphizable_type_argument(
                        &arg.kind,
                        self.facts.definitions(),
                    )
                })
            })
            .filter(|args| {
                seen.insert(Symbol::type_thunk(
                    crate::mir::symbol::ThunkKind::Drop,
                    type_name,
                    *args,
                ))
            })
            .map(Vec::as_slice)
            .collect()
    }

    /// Generate the decref thunk for every structural collection entry the
    /// program uses.
    ///
    /// A collection releases the entries it discards through a drop callback,
    /// which takes the address of a decref function. An entry that is a named
    /// type already has one; a tuple, an option or a function value has no
    /// declaration to name, so its thunk is generated here, keyed by the entry
    /// type's structure.
    fn generate_structural_element_decref_functions(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
        bodies: &[(&str, &Body)],
    ) -> Result<(), CodegenError> {
        let mut matched_by_fields = std::collections::BTreeMap::new();
        for element in structural_elements::structural_element_types(bodies) {
            FunctionTranslator::generate_structural_decref_function(
                module,
                ctx,
                isa,
                &element,
                &self.facts,
            )?;
            let scope = LayoutScope::new(self.facts.definitions(), &element.open_params);
            let layers = OptionalLayers::of(&element.kind, scope);
            let matched = layers.matched_kind(&element.kind);
            if let Some((encoding, layout)) =
                matched.and_then(|kind| fieldwise_value_identity(kind, scope))
            {
                matched_by_fields.insert(encoding, layout);
            }
        }
        for (encoding, layout) in matched_by_fields {
            value_identity_thunks::generate_value_identity_thunks(
                module, ctx, isa, &encoding, &layout,
            )?;
        }
        Ok(())
    }

    /// Generate a per-instantiation `miri.TypeName$Args.$drop` thunk for each
    /// instantiation of a generic struct, class or enum a value is released at
    /// ([`TypeFacts::drop_instantiations_of`]), so a managed field is DecRef'd
    /// and a scalar field skipped, each per instantiation. Non-generic types
    /// and types no value is held at produce nothing here (the bare thunk
    /// suffices).
    fn generate_instantiation_drop_functions(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
    ) -> Result<(), CodegenError> {
        let Some(definition) = self.facts.definitions().get(type_name) else {
            return Ok(());
        };
        if definition.generics().is_none() {
            return Ok(());
        }
        // The drop set holds each thunk symbol once, and refuses two
        // instantiations meeting in one symbol when it is settled.
        for args in self.facts.drop_instantiations_of(type_name) {
            FunctionTranslator::generate_drop_function(
                module,
                ctx,
                isa,
                type_name,
                Some(args),
                &self.facts,
            )?;
            // Per-instantiation `miri.Box$String.$decref` wrapper: the collection
            // element decref helper for a `List<Box<String>>` must route to the
            // per-instantiation drop thunk so the concrete managed field is
            // released. The bare `miri.Box.$decref` would reach only `miri.Box.$drop`,
            // which skips the unresolved generic field.
            FunctionTranslator::generate_decref_function(
                module,
                ctx,
                isa,
                type_name,
                args,
                rc::Release::of(type_name, &self.facts),
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CraneliftBackend(target={})", self.isa.triple())
    }
}

/// Defines `func_id` from the function in `ctx`, with every call into a Miri
/// runtime library made direct ([`call_runtime_directly`]). Every function the
/// backend emits is defined through here.
pub(crate) fn define_function(
    module: &mut ObjectModule,
    func_id: cranelift_module::FuncId,
    ctx: &mut Context,
) -> Result<(), Box<cranelift_module::ModuleError>> {
    call_runtime_directly(&mut ctx.func, module.declarations());
    module.define_function(func_id, ctx).map_err(Box::new)
}

/// Marks every Miri runtime function `func` calls as colocated, so the
/// call is a direct branch the static linker resolves rather than a load
/// of the callee's address from the GOT and an indirect branch. The
/// runtime libraries are linked into every executable, well within a
/// direct branch's reach; anything else imported, such as the C library,
/// keeps the call position-independent code makes.
fn call_runtime_directly(
    func: &mut cranelift_codegen::ir::Function,
    declarations: &cranelift_module::ModuleDeclarations,
) {
    let runtime: Vec<cranelift_codegen::ir::FuncRef> = (func.dfg.ext_funcs.iter())
        .filter_map(|(func_ref, data)| {
            let id = user_function_id(func, &data.name)?;
            let decl = declarations.get_function_decl(id);
            let is_runtime = decl.linkage == Linkage::Import
                && decl
                    .linkage_name(id)
                    .starts_with(crate::runtime_fns::RUNTIME_SYMBOL_PREFIX);
            is_runtime.then_some(func_ref)
        })
        .collect();
    for func_ref in runtime {
        func.dfg.ext_funcs[func_ref].colocated = true;
    }
}

/// The structural symbol suffix and layout of `kind` when a set or map
/// matches it through the field-wise `equals` and `hash` thunks: a tuple or
/// an optional held as its bytes whose bytes are not its value. `None` for
/// every other kind.
pub(crate) fn fieldwise_value_identity(
    kind: &TypeKind,
    scope: LayoutScope<'_>,
) -> Option<(String, crate::mir::type_facts::ValueLayout)> {
    let layout = crate::mir::type_facts::held_value_layout(kind, scope)?;
    if layout.is_compared_by_bytes() {
        return None;
    }
    let encoding = structural_elements::structural_thunk_symbol(kind)?;
    Some((encoding, layout))
}

#[cfg(test)]
mod string_literal_tests {
    use super::CraneliftBackend;

    /// The word at `index` of a literal laid out for `ptr_size`-byte words.
    fn word(bytes: &[u8], index: usize, ptr_size: usize) -> u64 {
        let word = &bytes[index * ptr_size..(index + 1) * ptr_size];
        if ptr_size == 4 {
            u64::from(u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        } else {
            u64::from_ne_bytes([
                word[0], word[1], word[2], word[3], word[4], word[5], word[6], word[7],
            ])
        }
    }

    /// A literal is `[RC][data][len][capacity][ascii]`, matching the runtime's
    /// string header one word past the RC word. The rest of the ascii word is
    /// zero: the runtime reads its first byte past the flag as the storage
    /// kind, where zero means bytes the string does not own, and its second as
    /// the block's inline room, where zero means none.
    #[test]
    fn literal_layout_matches_the_runtime_string_header() {
        for ptr_size in [4usize, 8] {
            let bytes = CraneliftBackend::build_miri_string_struct_bytes(5, true, ptr_size as u32);
            assert_eq!(bytes.len(), 5 * ptr_size);
            assert_eq!(word(&bytes, 0, ptr_size), 1u64 << (8 * ptr_size - 1));
            assert_eq!(word(&bytes, 1, ptr_size), 0, "data is a relocation");
            assert_eq!(word(&bytes, 2, ptr_size), 5, "len");
            assert_eq!(word(&bytes, 3, ptr_size), 5, "capacity");
            let ascii_word = &bytes[4 * ptr_size..];
            assert_eq!(ascii_word[0], 1, "ascii");
            assert!(
                ascii_word[1..].iter().all(|&byte| byte == 0),
                "storage kind and inline room must be zero"
            );
        }
    }
}
