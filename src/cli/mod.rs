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
pub mod skill;
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

/// Build a diagnostic that carries a registry code but no source position.
///
/// A command-invocation failure has no span to point at: the fault is in what
/// was asked for, not in a place in a file. The code and the help line are what
/// make it actionable instead.
pub fn coded(
    code: crate::diagnostics::DiagnosticCode,
    message: String,
    help: &str,
) -> Box<crate::error::diagnostic::Diagnostic> {
    Box::new(
        crate::error::diagnostic::DiagnosticBuilder::error(code.title().to_string())
            .code(code.as_str())
            .message(message)
            .help(help.to_string())
            .build(),
    )
}

pub use args::{
    AgentFlavor, BuildTarget, Cli, ColorMode, Commands, CpuBackend, DeterminismCommand, Format,
    SkillCommand,
};
pub use version::{crate_version, version_ref, version_string};
