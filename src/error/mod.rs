// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Error and diagnostic types for the Miri compiler.
//!
//! This module provides unified error and warning handling with consistent
//! formatting across all compiler phases.

pub mod codegen;
pub mod compiler;
pub mod diagnostic;
pub mod format;
pub mod lowering;
pub mod runtime;
pub mod syntax;
pub mod type_error;

pub use codegen::CodegenError;
pub use compiler::CompilerError;
pub use diagnostic::{
    Diagnostic, DiagnosticBuilder, ErrorProperties, Reportable, Severity, BUG_REPORT_URL,
};
pub use format::{
    find_best_match, format_diagnostic, format_diagnostic_full, levenshtein_distance, ColorScheme,
};
pub use lowering::{LoweringError, LoweringErrorKind};
pub use runtime::RuntimeError;
pub use syntax::{find_line_info, Span, SyntaxError, SyntaxErrorKind};
pub use type_error::{TypeError, TypeErrorKind};
