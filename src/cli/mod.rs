// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod agent;
pub mod args;
pub mod check;
pub mod dev;
pub mod explain;
pub mod fix;
pub mod version;

use crate::diagnostics::json::DiagnosticsEnvelope;

/// Render an envelope for printing.
///
/// Serialization of a plain data envelope cannot realistically fail, but the
/// commands print rather than propagate, so a failure degrades to an object of
/// the same shape instead of taking the process down over a reporting concern.
pub fn serialize_envelope(envelope: &DiagnosticsEnvelope) -> String {
    serde_json::to_string_pretty(envelope)
        .unwrap_or_else(|error| format!("{{\"error\":\"could not serialize: {}\"}}", error))
}

pub use args::{BuildTarget, Cli, ColorMode, Commands, CpuBackend, Format};
pub use version::{version_ref, version_string};
