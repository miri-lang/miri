// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Cranelift code generation backend.
//!
//! This module provides the Cranelift-based code generator for CPU targets.
//! Cranelift is a fast code generator suitable for both JIT and AOT compilation.

mod closure;
mod gpu_launch;
pub mod layout;
mod predicates;
mod rc;
pub mod translate_rvalue;
pub mod translate_statement;
mod translator;
mod types;
mod vtable;

use crate::ast::types::BuiltinCollectionKind;
use crate::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
use crate::codegen::cranelift::translator::needs_out_pointer;
use crate::error::CodegenError;
use crate::mir::Body;
use crate::type_checker::context::TypeDefinition;
use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::Signature;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use target_lexicon::{DeploymentTarget, OperatingSystem, Triple};

pub use translator::FunctionTranslator;
pub use types::translate_type;

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
    /// Type definitions from the type checker (for layout computation).
    type_definitions: HashMap<String, TypeDefinition>,
    /// Runtime function imports to declare as external symbols.
    runtime_imports: Vec<RuntimeImport>,
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

impl Default for CraneliftOptions {
    fn default() -> Self {
        Self {
            opt_level: OptLevel::None,
            pic: true,
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
        let mut settings_builder = settings::builder();

        settings_builder
            .set("opt_level", "none")
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?;

        settings_builder
            .set("is_pic", "true")
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?;

        let flags = settings::Flags::new(settings_builder);

        let isa = cranelift_codegen::isa::lookup(target)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?;

        Ok(Self {
            isa,
            type_definitions: HashMap::new(),
            runtime_imports: Vec::new(),
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

    /// Set the type definitions for layout computation.
    pub fn set_type_definitions(&mut self, defs: HashMap<String, TypeDefinition>) {
        self.type_definitions = defs;
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
        self.generate_lambda_destructors(&mut module, &mut ctx, &isa, bodies)?;
        let kernel_registry =
            crate::codegen::cranelift::gpu_launch::build_kernel_registry(&mut module, bodies)?;
        let cpu_bodies: Vec<(&str, &Body)> = bodies
            .iter()
            .copied()
            .filter(|(_, b)| b.execution_model != crate::mir::ExecutionModel::GpuKernel)
            .collect();
        self.predeclare_user_functions(&mut module, &isa, &cpu_bodies)?;

        let mut string_literals = HashMap::new();
        for (name, body) in &cpu_bodies {
            self.compile_function(
                &mut module,
                &mut ctx,
                name,
                body,
                &isa,
                &mut string_literals,
                &kernel_registry,
            )?;
        }

        FunctionTranslator::generate_vtables(&mut module, &isa, &self.type_definitions)?;
        Self::define_string_literals(&mut module, &isa, string_literals)?;
        let object = self.finalize_object(module)?;

        Ok(CompiledArtifact {
            bytes: object,
            format: ArtifactFormat::ObjectFile,
        })
    }

    fn name(&self) -> &'static str {
        "cranelift"
    }
}

impl CraneliftBackend {
    /// Resolve the target ISA. Reuses the cached default ISA when options match
    /// defaults; otherwise rebuilds with the requested opt-level/PIC flags.
    fn resolve_isa(&self, options: &CraneliftOptions) -> Result<Arc<dyn TargetIsa>, CodegenError> {
        let is_default = options.opt_level == OptLevel::None && options.pic;
        if is_default {
            return Ok(self.isa.clone());
        }
        let mut settings_builder = settings::builder();
        let opt_level_str = match options.opt_level {
            OptLevel::None => "none",
            OptLevel::Speed => "speed",
            OptLevel::SpeedAndSize => "speed_and_size",
        };
        settings_builder
            .set("opt_level", opt_level_str)
            .map_err(|e| CodegenError::Module(e.to_string()))?;
        if options.pic {
            settings_builder
                .set("is_pic", "true")
                .map_err(|e| CodegenError::Module(e.to_string()))?;
        }
        let flags = settings::Flags::new(settings_builder);
        cranelift_codegen::isa::lookup(self.isa.triple().clone())
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))
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

    /// Generate `__dtor_{lambda_name}` destructors for closure bodies that
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
                crate::codegen::cranelift::translator::is_capture_managed(
                    &body.local_decls[cap_local.0].ty.kind,
                )
            });
            if has_managed {
                FunctionTranslator::generate_closure_destructor(
                    module,
                    ctx,
                    isa,
                    name,
                    body,
                    &self.type_definitions,
                )?;
            }
        }
        Ok(())
    }

    /// Pre-declare all user functions with MIR-derived signatures so call sites
    /// compiled before the callee resolve to the correct signature (avoids
    /// the DFG-inferred widened-type mismatch).
    fn predeclare_user_functions(
        &self,
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        bodies: &[(&str, &Body)],
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        for (name, body) in bodies.iter() {
            let mut sig = Signature::new(call_conv);
            if !body.local_decls.is_empty() {
                let ret_ty = &body.local_decls[0].ty;
                if ret_ty.kind != crate::ast::types::TypeKind::Void {
                    sig.returns
                        .push(AbiParam::new(translate_type(ret_ty, ptr_type)));
                }
            }
            // Scalar `out` params use ptr_type (copy-in/copy-out ABI) — must match
            // the signature built in FunctionTranslator::build_signature.
            for i in 1..=body.arg_count {
                if i < body.local_decls.len() {
                    let param_ty = &body.local_decls[i].ty;
                    let cl_type = if body.out_params.get(i - 1).copied().unwrap_or(false)
                        && needs_out_pointer(&param_ty.kind)
                    {
                        ptr_type
                    } else {
                        translate_type(param_ty, ptr_type)
                    };
                    sig.params.push(AbiParam::new(cl_type));
                }
            }
            module
                .declare_function(name, Linkage::Export, &sig)
                .map_err(|e| CodegenError::declare_function(*name, e.to_string()))?;
        }
        Ok(())
    }

    /// Define collected string literals as immortal static data structures
    /// (`[RC][DataPtr][Len][Cap]`) referencing a sibling `*_bytes` data symbol.
    fn define_string_literals(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        string_literals: HashMap<String, String>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        for (literal, symbol_name) in string_literals {
            Self::define_one_string_literal(module, &literal, &symbol_name, ptr_size)?;
        }
        Ok(())
    }

    /// Emit the byte data + MiriString struct for one string literal.
    fn define_one_string_literal(
        module: &mut ObjectModule,
        literal: &str,
        symbol_name: &str,
        ptr_size: u32,
    ) -> Result<(), CodegenError> {
        let bytes_id = Self::define_string_bytes(module, literal, symbol_name)?;

        let mut struct_symbol = String::with_capacity(symbol_name.len() + 7);
        struct_symbol.push_str(symbol_name);
        struct_symbol.push_str("_struct");
        let struct_id = module
            .declare_data(&struct_symbol, Linkage::Export, false, false)
            .map_err(|e| CodegenError::Module(e.to_string()))?;

        let mut struct_ctx = DataDescription::new();
        struct_ctx.set_align(ptr_size as u64);
        struct_ctx.define(
            Self::build_miri_string_struct_bytes(literal.len() as u64, ptr_size).into_boxed_slice(),
        );

        // Relocation for the data pointer at offset ptr_size
        let bytes_ref = module.declare_data_in_data(bytes_id, &mut struct_ctx);
        struct_ctx.write_data_addr(ptr_size, bytes_ref, 0);

        module
            .define_data(struct_id, &struct_ctx)
            .map_err(|e| CodegenError::Module(e.to_string()))
    }

    /// Define the raw byte data for a string literal as
    /// `{symbol_name}_bytes`; return the `DataId` of the byte array.
    fn define_string_bytes(
        module: &mut ObjectModule,
        literal: &str,
        symbol_name: &str,
    ) -> Result<cranelift_module::DataId, CodegenError> {
        let mut bytes_symbol = String::with_capacity(symbol_name.len() + 6);
        bytes_symbol.push_str(symbol_name);
        bytes_symbol.push_str("_bytes");
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

    /// Build the `MiriString` struct payload: `[RC | DataPtr | Len | Cap]`
    /// with the high RC bit set so the runtime treats the literal as immortal.
    /// The DataPtr slot (offset `ptr_size`) is left zero — the caller writes
    /// the relocation via `write_data_addr`.
    fn build_miri_string_struct_bytes(len: u64, ptr_size: u32) -> Vec<u8> {
        let mut data = vec![0u8; 4 * ptr_size as usize];
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

    /// Compile a single MIR function body to Cranelift IR.
    ///
    /// # Arguments
    /// * `module` - The ObjectModule being built.
    /// * `ctx` - The Cranelift Context for this function.
    /// * `name` - The symbol name of the function.
    /// * `body` - The MIR Body to translate.
    /// * `isa` - The TargetIsa for code generation.
    /// * `string_literals` - A map to collect and deduplicate string literals found in the function.
    #[allow(clippy::too_many_arguments)]
    fn compile_function(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        name: &str,
        body: &Body,
        isa: &Arc<dyn TargetIsa>,
        string_literals: &mut HashMap<String, String>,
        kernel_registry: &HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
    ) -> Result<(), CodegenError> {
        // Create function translator
        let mut translator = FunctionTranslator::new(isa, body, &self.type_definitions);

        // Translate MIR to Cranelift IR
        translator
            .translate(body, module, string_literals, kernel_registry)
            .map_err(|e| CodegenError::translation(name, e.to_string()))?;

        // Get the function signature and declare it
        let sig = translator.signature().clone();
        let func_id = module
            .declare_function(name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;

        // Set the function in context
        ctx.func = translator.into_function();

        // Define the function
        module.define_function(func_id, ctx).map_err(|e| {
            // Include the Cranelift IR in the error message for diagnostics.
            // This replaces direct println! to keep output channels clean.
            CodegenError::define_function(
                name,
                format!("{}\n\nCranelift IR:\n{}", e, ctx.func.display()),
            )
        })?;

        // Clear context for next function
        ctx.clear();

        Ok(())
    }

    /// Generates `__drop_TypeName(ptr)` functions for every managed concrete type.
    ///
    /// A type is "managed" if it is a non-generic struct, class, or enum that has
    /// at least one field of a managed (heap-allocated) type. These drop functions
    /// form the foundation of the Perceus RC destructor pipeline:
    ///
    ///   RC reaches 0 → call `__drop_TypeName(ptr)`
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
        // We include types without managed fields so that `__decref_TypeName` can be
        // generated for them — it is needed as elem_drop_fn when such types are
        // stored in a List, Set, or Map. Generic Classes are accepted: the drop
        // thunk is keyed by bare type name (no generic args mangled in), so one
        // thunk serves every instantiation. Struct/Enum stay non-generic because
        // their field DecRef sequences may depend on element layout.
        //
        // Builtin collection class names (`List`, `Map`, `Set`, `Array`, `Tuple`)
        // and `String` are skipped: their drop / decref / clone paths route through
        // dedicated runtime helpers (`miri_rt_list_free`, …) inside `emit_type_drop`
        // before any `__drop_TypeName` thunk is consulted, so emitting one would be
        // dead code that bloats the object file.
        let mut managed_names: Vec<&str> = self
            .type_definitions
            .iter()
            .filter_map(|(name, def)| {
                let skip_generic = match def {
                    TypeDefinition::Struct(sd) => sd.generics.is_some(),
                    TypeDefinition::Class(_) => false,
                    TypeDefinition::Enum(ed) => ed.generics.is_some(),
                    TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_)
                    | TypeDefinition::Trait(_) => return None,
                };
                if skip_generic {
                    return None;
                }
                if BuiltinCollectionKind::from_name(name).is_some()
                    || name == "String"
                    || name == "Tuple"
                {
                    return None;
                }
                Some(name.as_str())
            })
            .collect();
        managed_names.sort_unstable();

        for type_name in managed_names {
            FunctionTranslator::generate_drop_function(
                module,
                ctx,
                isa,
                type_name,
                &self.type_definitions,
            )?;
            FunctionTranslator::generate_clone_function(
                module,
                ctx,
                isa,
                type_name,
                &self.type_definitions,
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
