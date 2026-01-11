// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Cranelift code generation backend.
//!
//! This module provides the Cranelift-based code generator for CPU targets.
//! Cranelift is a fast code generator suitable for both JIT and AOT compilation.

mod translator;
mod types;

use crate::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
use crate::error::CodegenError;
use crate::mir::Body;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::fmt;
use std::sync::Arc;
use target_lexicon::{DeploymentTarget, OperatingSystem, Triple};

pub use translator::FunctionTranslator;
pub use types::translate_type;

/// Cranelift backend for native code generation.
///
/// This backend uses Cranelift to generate machine code for the host platform.
/// It supports AOT compilation to object files which can be linked into executables.
pub struct CraneliftBackend {
    /// The target ISA (instruction set architecture).
    isa: Arc<dyn TargetIsa>,
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
            pic: false,
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

        let flags = settings::Flags::new(settings_builder);

        let isa = cranelift_codegen::isa::lookup(target)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?;

        Ok(Self { isa })
    }

    /// Get the target triple this backend is configured for.
    pub fn target(&self) -> &Triple {
        self.isa.triple()
    }
}

impl Default for CraneliftBackend {
    fn default() -> Self {
        Self::new().expect("Failed to create Cranelift backend for host platform")
    }
}

impl Backend for CraneliftBackend {
    type Error = CodegenError;
    type Options = CraneliftOptions;

    fn compile(
        &self,
        bodies: &[(&str, &Body)],
        options: &Self::Options,
    ) -> Result<CompiledArtifact, Self::Error> {
        // Create module settings based on options
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

        // Rebuild ISA with new flags
        let isa = cranelift_codegen::isa::lookup(self.isa.triple().clone())
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CodegenError::TargetIsa(e.to_string()))?;

        // Create object module
        let object_builder = ObjectBuilder::new(
            isa.clone(),
            "miri_module",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CodegenError::Module(e.to_string()))?;

        let mut module = ObjectModule::new(object_builder);
        let mut ctx = Context::new();

        // Compile each function
        for (name, body) in bodies {
            self.compile_function(&mut module, &mut ctx, name, body, &isa)?;
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
    fn compile_function(
        &self,
        module: &mut ObjectModule,
        ctx: &mut Context,
        name: &str,
        body: &Body,
        isa: &Arc<dyn TargetIsa>,
    ) -> Result<(), CodegenError> {
        // Create function translator
        let mut translator = FunctionTranslator::new(isa, body);

        // Translate MIR to Cranelift IR
        translator
            .translate(body)
            .map_err(|e| CodegenError::translation(name, e))?;

        // Get the function signature and declare it
        let sig = translator.signature().clone();
        let func_id = module
            .declare_function(name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;

        // Set the function in context
        ctx.func = translator.into_function();

        // Define the function
        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(name, e.to_string()))?;

        // Clear context for next function
        ctx.clear();

        Ok(())
    }
}

impl fmt::Display for CraneliftBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CraneliftBackend(target={})", self.isa.triple())
    }
}
