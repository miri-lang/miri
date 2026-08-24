// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic codes and severity levels for the Miri compiler.
//!
//! This module provides the stable diagnostic code registry and severity definitions.
//! It is the inner layer; `src/error/` imports from here, never the reverse.

pub mod codes;
pub mod severity;

pub use codes::DiagnosticCode;
pub use severity::Severity;
