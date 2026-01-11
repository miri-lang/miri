// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Error and diagnostic types for the Miri compiler.
//!
//! This module provides unified error and warning handling with consistent
//! formatting across all compiler phases.

pub mod codegen;
pub mod codes;
pub mod compiler;
pub mod diagnostic;
pub mod format;
pub mod interpreter;
pub mod lowering;
pub mod runtime;
pub mod syntax;
pub mod type_error;

pub use codegen::*;
pub use compiler::*;
pub use diagnostic::*;
pub use format::*;
pub use interpreter::*;
pub use lowering::*;
pub use runtime::*;
pub use syntax::*;
pub use type_error::*;
