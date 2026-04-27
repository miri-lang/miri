// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Cranelift code generation backend.
//!
//! This module provides the Cranelift-based code generator for CPU targets.
//! Cranelift is a fast code generator suitable for both JIT and AOT compilation.

pub mod layout;
pub mod translate_rvalue;
pub mod translate_statement;
mod translator;
mod types;

use crate::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
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
    /// This method translates each body into Cranelift IR, manages string literals
    /// by emitting them as static data, and generates a native object file.
    fn compile(
        &self,
        bodies: &[(&str, &Body)],
        options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        // Reuse the existing ISA when options match defaults (avoids ISA rebuild).
        let is_default = options.opt_level == OptLevel::None && options.pic;
        let isa = if is_default {
            self.isa.clone()
        } else {
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
                .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
        };

        // Create object module
        let object_builder = ObjectBuilder::new(
            isa.clone(),
            "miri_module",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CodegenError::Module(e.to_string()))?;

        let mut module = ObjectModule::new(object_builder);
        let mut ctx = Context::new();

        // Declare runtime function imports as external symbols
        self.declare_runtime_imports(&mut module)?;

        // Generate type-specific `__drop_TypeName` functions for every managed
        // concrete type (structs, classes, enums with managed fields).
        // These must be defined before user functions so that Import declarations
        // inside user code resolve correctly.
        self.generate_type_drop_functions(&mut module, &mut ctx, &isa)
            .map_err(|e| CodegenError::Module(format!("drop thunk generation: {e}")))?;

        // Generate `__dtor_{lambda_name}` destructors for lambda bodies that have
        // managed captures. These must be defined before user functions so that
        // translate_closure_aggregate can reference them via Linkage::Import.
        for (name, body) in bodies.iter() {
            if !body.env_capture_locals.is_empty() {
                let has_managed = body.env_capture_locals.iter().any(|&cap_local| {
                    crate::codegen::cranelift::translator::is_capture_managed(
                        &body.local_decls[cap_local.0].ty.kind,
                    )
                });
                if has_managed {
                    FunctionTranslator::generate_closure_destructor(
                        &mut module,
                        &mut ctx,
                        &isa,
                        name,
                        body,
                        &self.type_definitions,
                    )
                    .map_err(|e| CodegenError::Module(format!("closure dtor generation: {e}")))?;
                }
            }
        }

        // Pre-declare all user functions with correct signatures from MIR types.
        // This prevents signature mismatches when a call site is compiled before
        // the callee's definition (the call site would otherwise infer the
        // signature from DFG value types which may be widened).
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        for (name, body) in bodies.iter() {
            let mut sig = Signature::new(call_conv);
            // Return type is local 0
            if !body.local_decls.is_empty() {
                let ret_ty = &body.local_decls[0].ty;
                if ret_ty.kind != crate::ast::types::TypeKind::Void {
                    sig.returns
                        .push(AbiParam::new(translate_type(ret_ty, ptr_type)));
                }
            }
            // Parameters are locals 1..=arg_count
            for i in 1..=body.arg_count {
                if i < body.local_decls.len() {
                    let param_ty = &body.local_decls[i].ty;
                    sig.params
                        .push(AbiParam::new(translate_type(param_ty, ptr_type)));
                }
            }
            module
                .declare_function(name, Linkage::Export, &sig)
                .map_err(|e| CodegenError::declare_function(*name, e.to_string()))?;
        }

        // Compile each function
        let mut string_literals = HashMap::new();
        for (name, body) in bodies {
            self.compile_function(
                &mut module,
                &mut ctx,
                name,
                body,
                &isa,
                &mut string_literals,
            )?;
        }

        // Generate vtables for classes that participate in virtual dispatch.
        // Must run after all user functions are compiled so method symbols are registered.
        FunctionTranslator::generate_vtables(&mut module, &isa, &self.type_definitions)
            .map_err(|e| CodegenError::Module(format!("vtable generation: {e}")))?;

        // Define string literals as static data structures
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        for (literal, symbol_name) in string_literals {
            // 1. Define the raw bytes
            let mut bytes_symbol = String::with_capacity(symbol_name.len() + 6);
            bytes_symbol.push_str(&symbol_name);
            bytes_symbol.push_str("_bytes");
            let bytes_id = module
                .declare_data(&bytes_symbol, Linkage::Export, false, false)
                .map_err(|e| CodegenError::Module(e.to_string()))?;
            let mut bytes_ctx = DataDescription::new();
            bytes_ctx.define(literal.as_bytes().to_vec().into_boxed_slice());
            module
                .define_data(bytes_id, &bytes_ctx)
                .map_err(|e| CodegenError::Module(e.to_string()))?;

            // 2. Define the MiriString struct: [RC, DataPtr, Len, Cap]
            let mut struct_symbol = String::with_capacity(symbol_name.len() + 7);
            struct_symbol.push_str(&symbol_name);
            struct_symbol.push_str("_struct");
            let struct_id = module
                .declare_data(&struct_symbol, Linkage::Export, false, false)
                .map_err(|e| CodegenError::Module(e.to_string()))?;

            let mut struct_ctx = DataDescription::new();
            struct_ctx.set_align(ptr_size as u64);
            let mut data = vec![0u8; 4 * ptr_size as usize];

            // RC header: set high bit to indicate immortal/constant object
            let immortal_rc = if ptr_size == 4 {
                (1u32 << 31) as u64
            } else {
                1u64 << 63
            };

            if ptr_size == 4 {
                data[0..4].copy_from_slice(&(immortal_rc as u32).to_ne_bytes());
            } else {
                data[0..8].copy_from_slice(&immortal_rc.to_ne_bytes());
            }

            // Len and Cap (both same for literals)
            let len = literal.len() as u64;
            if ptr_size == 4 {
                data[8..12].copy_from_slice(&(len as u32).to_ne_bytes());
                data[12..16].copy_from_slice(&(len as u32).to_ne_bytes());
            } else {
                data[16..24].copy_from_slice(&len.to_ne_bytes());
                data[24..32].copy_from_slice(&len.to_ne_bytes());
            }

            struct_ctx.define(data.into_boxed_slice());

            // Relocation for the data pointer at offset ptr_size
            let bytes_ref = module.declare_data_in_data(bytes_id, &mut struct_ctx);
            struct_ctx.write_data_addr(ptr_size as u32, bytes_ref, 0);

            module
                .define_data(struct_id, &struct_ctx)
                .map_err(|e| CodegenError::Module(e.to_string()))?;
        }

        // Emit the object file
        let mut product = module.finish();

        // If we are on macOS (Darwin), we need to inject the Mach-O build version load command.
        // cranelift-object currently doesn't do this automatically even if the target is set correctly.
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

        let object = product
            .emit()
            .map_err(|e| CodegenError::Emit(e.to_string()))?;

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
    /// Compile a single MIR function body to Cranelift IR.
    ///
    /// # Arguments
    /// * `module` - The ObjectModule being built.
    /// * `ctx` - The Cranelift Context for this function.
    /// * `name` - The symbol name of the function.
    /// * `body` - The MIR Body to translate.
    /// * `isa` - The TargetIsa for code generation.
    /// * `string_literals` - A map to collect and deduplicate string literals found in the function.
    fn compile_function(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        name: &str,
        body: &Body,
        isa: &Arc<dyn TargetIsa>,
        string_literals: &mut HashMap<String, String>,
    ) -> Result<(), CodegenError> {
        // Create function translator
        let mut translator = FunctionTranslator::new(isa, body, &self.type_definitions);

        // Translate MIR to Cranelift IR
        translator
            .translate(body, module, string_literals)
            .map_err(|e| CodegenError::translation(name, e))?;

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
    ///     → (1) user-defined drop hook (no-op placeholder for M5 Task 3)
    ///     → (2) recursively DecRef all managed fields
    ///     → (3) free the RC allocation
    ///
    /// Types with no managed fields do not get a thunk; their drop path calls
    /// `libc::free` directly from `emit_type_drop`.
    fn generate_type_drop_functions(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        isa: &Arc<dyn TargetIsa>,
    ) -> Result<(), String> {
        // Collect all non-generic concrete Struct/Class/Enum types and sort for
        // deterministic output.  We include types without managed fields so that
        // __decref_TypeName can be generated for them — it is needed as elem_drop_fn
        // when such types are stored in a List, Set, or Map.
        let mut managed_names: Vec<&str> = self
            .type_definitions
            .iter()
            .filter_map(|(name, def)| {
                // Skip generic types — they are not concrete instantiations.
                let has_generics = match def {
                    TypeDefinition::Struct(sd) => sd.generics.is_some(),
                    TypeDefinition::Class(cd) => cd.generics.is_some(),
                    TypeDefinition::Enum(ed) => ed.generics.is_some(),
                    _ => return None,
                };
                if has_generics {
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
        }
        Ok(())
    }
}

impl fmt::Display for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CraneliftBackend(target={})", self.isa.triple())
    }
}
