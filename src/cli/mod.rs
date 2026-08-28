// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod agent;
pub mod args;
pub mod check;
pub mod determinism;
pub mod dev;
pub mod explain;
pub mod fix;
pub mod patch;
pub mod resolve;
pub mod token_align;
pub mod version;
pub mod view;

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

/// Make an arbitrary argument safe to echo to a terminal.
///
/// The rejected argument is quoted back to the user, and it is entirely under
/// their control. Escape sequences passed straight through would let a crafted
/// argument repaint or rewrite the surrounding terminal output, so control
/// characters are shown as escapes rather than executed. Printable text of any
/// script is left alone. JSON needs no such treatment: the serializer already
/// escapes control characters.
pub fn sanitize_for_terminal(argument: &str) -> String {
    argument
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

pub use args::{BuildTarget, Cli, ColorMode, Commands, CpuBackend, DeterminismCommand, Format};
pub use version::{version_ref, version_string};
