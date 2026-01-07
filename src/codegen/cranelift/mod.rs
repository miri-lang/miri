// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Cranelift code generation backend.
//!
//! This module provides the Cranelift-based code generator for CPU targets.
//! Cranelift is a fast code generator suitable for both JIT and AOT compilation.

mod translator;
mod types;

use crate::codegen::backend::{ArtifactFormat, Backend, CompiledArtifact};
use crate::mir::Body;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::fmt;
use std::sync::Arc;
use target_lexicon::Triple;
use thiserror::Error;

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

/// Errors from the Cranelift backend.
#[derive(Debug, Error)]
pub enum CraneliftError {
    /// Failed to create target ISA.
    #[error("Failed to create target ISA: {0}")]
    TargetIsa(String),

    /// Failed to create module.
    #[error("Failed to create module: {0}")]
    Module(String),

    /// Failed to declare function.
    #[error("Failed to declare function '{0}': {1}")]
    DeclareFunction(String, String),

    /// Failed to define function.
    #[error("Failed to define function '{0}': {1}")]
    DefineFunction(String, String),

    /// Failed to translate MIR to Cranelift IR.
    #[error("Failed to translate function '{0}': {1}")]
    Translation(String, String),

    /// Failed to emit object file.
    #[error("Failed to emit object file: {0}")]
    Emit(String),
}

impl CraneliftBackend {
    /// Create a new Cranelift backend for the host platform.
    pub fn new() -> Result<Self, CraneliftError> {
        Self::for_target(Triple::host())
    }

    /// Create a new Cranelift backend for a specific target.
    pub fn for_target(target: Triple) -> Result<Self, CraneliftError> {
        let mut settings_builder = settings::builder();

        settings_builder
            .set("opt_level", "none")
            .map_err(|e| CraneliftError::TargetIsa(e.to_string()))?;

        let flags = settings::Flags::new(settings_builder);

        let isa = cranelift_codegen::isa::lookup(target)
            .map_err(|e| CraneliftError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CraneliftError::TargetIsa(e.to_string()))?;

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
    type Error = CraneliftError;
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
            .map_err(|e| CraneliftError::Module(e.to_string()))?;

        if options.pic {
            settings_builder
                .set("is_pic", "true")
                .map_err(|e| CraneliftError::Module(e.to_string()))?;
        }

        let flags = settings::Flags::new(settings_builder);

        // Rebuild ISA with new flags
        let isa = cranelift_codegen::isa::lookup(self.isa.triple().clone())
            .map_err(|e| CraneliftError::TargetIsa(e.to_string()))?
            .finish(flags)
            .map_err(|e| CraneliftError::TargetIsa(e.to_string()))?;

        // Create object module
        let object_builder = ObjectBuilder::new(
            isa.clone(),
            "miri_module",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| CraneliftError::Module(e.to_string()))?;

        let mut module = ObjectModule::new(object_builder);
        let mut ctx = Context::new();

        // Compile each function
        for (name, body) in bodies {
            self.compile_function(&mut module, &mut ctx, name, body, &isa)?;
        }

        // Emit the object file
        let object = module
            .finish()
            .emit()
            .map_err(|e| CraneliftError::Emit(e.to_string()))?;

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
    ) -> Result<(), CraneliftError> {
        // Create function translator
        let mut translator = FunctionTranslator::new(isa, body);

        // Translate MIR to Cranelift IR
        translator
            .translate(body)
            .map_err(|e| CraneliftError::Translation(name.to_string(), e))?;

        // Get the function signature and declare it
        let sig = translator.signature().clone();
        let func_id = module
            .declare_function(name, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::DeclareFunction(name.to_string(), e.to_string()))?;

        // Set the function in context
        ctx.func = translator.into_function();

        // Define the function
        module
            .define_function(func_id, ctx)
            .map_err(|e| CraneliftError::DefineFunction(name.to_string(), e.to_string()))?;

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
