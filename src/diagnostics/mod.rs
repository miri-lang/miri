// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic codes and severity levels for the Miri compiler.
//!
//! This module provides the stable diagnostic code registry and severity definitions.
//! It is the inner layer; `src/error/` imports from here, never the reverse.

pub mod codes;
pub mod explain;
pub mod fix_safety;
pub mod json;
pub mod refusal;
pub mod repair;
pub mod rpc;
pub mod severity;

pub use codes::DiagnosticCode;
pub use explain::Explanation;
pub use fix_safety::FixSafety;
pub use json::DiagnosticsEnvelope;
pub use refusal::{compute_refused_repairs, repair_fix_safety, RefusedRepair};
pub use repair::{RepairId, RepairRequest};
pub use severity::Severity;
