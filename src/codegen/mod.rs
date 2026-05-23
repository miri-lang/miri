// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Code generation backends.
//!
//! This module provides the trait and implementations for code generation backends.
//! Currently supported backends:
//! - **Cranelift**: Fast compilation, good for development (default)
//! - **LLVM**: Optimized compilation (not yet implemented)

pub mod backend;
pub mod cranelift;
pub mod llvm;
pub mod web_gpu;
pub mod wgsl;

pub use backend::{ArtifactFormat, Backend, CompiledArtifact};
pub use cranelift::CraneliftBackend;
pub use llvm::LlvmBackend;
pub use wgsl::{WgslBackend, WgslOptions};
