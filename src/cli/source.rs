// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading the input file a command was pointed at.
//!
//! Every command that takes a path reads it through here, so a path that cannot
//! be opened is reported the same way whoever asked: one registry code, one
//! message, and one help line. Reporting it in each command separately is how
//! the same failure came to be a coded diagnostic in one place and a bare line
//! of prose in six others.

use std::path::Path;

use crate::cli::{coded, sanitize_for_terminal, serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{to_json, Diagnostic};
use crate::error::format::format_diagnostic_with_color;

/// Read the file a command was pointed at.
///
/// The read is attempted before anything is asked about the path: the happy
/// path costs one system call, and nothing can change underneath a decision
/// made from an answer that is already stale. Only once it has failed is the
/// path asked whether it is a directory, which is the one failure worth its
/// own sentence — it is what a caller gets for naming a project instead of a
/// file, and the help line is what tells them so.
pub fn read(path: &Path) -> Result<String, Box<Diagnostic>> {
    std::fs::read_to_string(path).map_err(|error| {
        if path.is_dir() {
            return unreadable(
                path,
                "it is a directory, not a file",
                "name a single .mi file; `miri test <dir>` is the command that reads a directory",
            );
        }
        unreadable(
            path,
            &error.to_string(),
            "check that the path exists, names a file rather than a directory, and is readable",
        )
    })
}

/// Report a path a command was pointed at that is not there.
///
/// A command that accepts a directory as readily as a file cannot learn the
/// path is wrong by trying to read it, so it asks here instead. The failure is
/// the one [`read`] reports and carries the same code, so a caller recognises
/// it without knowing which command looked.
pub fn missing(path: &Path) -> Box<Diagnostic> {
    unreadable(
        path,
        "no such file or directory",
        "check the path; `miri test` accepts either a .mi file or a directory to walk",
    )
}

/// Build the one diagnostic every unreadable input is reported as.
///
/// The path is echoed back to whoever asked and is entirely under their
/// control, so control characters are shown as escapes rather than executed —
/// a crafted path would otherwise repaint the terminal it is printed to.
fn unreadable(path: &Path, reason: &str, help: &str) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldInputNotReadable,
        format!(
            "could not read {}: {}",
            sanitize_for_terminal(&path.display().to_string()),
            reason
        ),
        help,
    )
}

/// Write out a file that could not be read, in the shape the caller asked for.
///
/// A caller that asked for JSON gets an envelope: answering a machine with a
/// bare line of prose would break the shape every other command promises. The
/// envelope carries the command that failed, so a consumer reads this the way
/// it reads any other refusal.
pub fn report_unreadable(
    path: &Path,
    diagnostic: &Diagnostic,
    command: JsonCommand,
    format: Format,
    color_mode: ColorMode,
) {
    let path_text = path.display().to_string();
    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(
                command,
                false,
                vec![to_json(diagnostic, "", Some(&path_text))],
            )
            .with_exit_code(1);
            println!("{}", serialize_envelope(&envelope));
        }
        Format::Pretty => eprint!(
            "{}",
            format_diagnostic_with_color("", diagnostic, Some(&path_text), color_mode.into())
        ),
    }
}

/// Read the file a command was pointed at, writing out any failure.
///
/// The two halves are also exposed separately because a caller that already
/// holds an envelope of its own reports the failure into that envelope rather
/// than printing a second one.
pub fn read_or_report(
    path: &Path,
    command: JsonCommand,
    format: Format,
    color_mode: ColorMode,
) -> Option<String> {
    match read(path) {
        Ok(source) => Some(source),
        Err(diagnostic) => {
            report_unreadable(path, &diagnostic, command, format, color_mode);
            None
        }
    }
}
