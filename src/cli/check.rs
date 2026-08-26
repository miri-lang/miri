// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri check` command: run the frontend and report what it found.
//!
//! The command is split so that the work and the writing are separate. [`check`]
//! runs the frontend and returns what it found; it prints nothing, exits
//! nothing, and can therefore be called by a long-lived server as readily as by
//! the command line. [`run`] adds the writing and is what the binary calls.
//!
//! Warnings never fail a check. The envelope carries `ok: true` whenever the
//! frontend succeeded, however many warnings it emitted; only an error makes it
//! false.

use std::path::Path;

use crate::cli::{serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
use crate::error::diagnostic::{to_json, Diagnostic};
use crate::error::format::format_diagnostic_with_color;
use crate::pipeline::Pipeline;

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The frontend succeeded. Warnings may still have been reported.
    Succeeded,
    /// The frontend reported at least one error.
    Failed,
}

/// What a check found.
///
/// The diagnostics are kept in their compiler form alongside the envelope
/// because the human-readable rendering needs the span and source context that
/// the JSON projection flattens away. Holding both is what lets one check
/// answer a machine and a person without running twice.
pub struct CheckReport {
    /// The envelope, ready to serialize for a machine consumer.
    pub envelope: DiagnosticsEnvelope,
    /// Whether the frontend succeeded. Warnings do not make this false.
    pub ok: bool,
    /// The diagnostics as the compiler reported them.
    diagnostics: Vec<Diagnostic>,
    /// The path the diagnostics were reported against.
    source_path: Option<String>,
}

/// Run the frontend over `source` and report what it found.
///
/// Nothing here writes to a stream or ends the process, so the same call serves
/// the command line and a request over a long-lived connection.
pub fn check(path: &Path, source: &str, verify_mir: bool) -> CheckReport {
    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    // Canonicalize first so that a bare filename like "main.mi" resolves to an
    // absolute path whose parent is the working directory, not an empty path.
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(directory) = absolute.parent() {
        pipeline = pipeline.with_source_dir(directory.to_path_buf());
    }
    pipeline = pipeline.with_source_path(absolute.display().to_string());

    let start = std::time::Instant::now();
    let outcome = pipeline.frontend(source);
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let source_path = pipeline.source_path().map(str::to_string);

    let (diagnostics, ok) = match outcome {
        Ok(result) => (result.type_checker.warnings().to_vec(), true),
        Err(error) => (error.to_diagnostics(), false),
    };

    let json_diagnostics = diagnostics
        .iter()
        .map(|diagnostic| to_json(diagnostic, source, source_path.as_deref()))
        .collect();

    let envelope = DiagnosticsEnvelope::new(JsonCommand::Check, ok, json_diagnostics)
        .with_exit_code(if ok { 0 } else { 1 })
        .with_duration_ms(elapsed_ms);

    CheckReport {
        envelope,
        ok,
        diagnostics,
        source_path,
    }
}

impl CheckReport {
    /// Serialize the report for a machine consumer.
    pub fn to_json(&self) -> String {
        serialize_envelope(&self.envelope)
    }

    /// Render each diagnostic with its source context, one per entry.
    pub fn rendered_diagnostics<'a>(
        &'a self,
        source: &'a str,
        color_mode: ColorMode,
    ) -> impl Iterator<Item = String> + 'a {
        self.diagnostics.iter().map(move |diagnostic| {
            format_diagnostic_with_color(
                source,
                diagnostic,
                self.source_path.as_deref(),
                color_mode.into(),
            )
        })
    }

    /// The closing line that says whether the check passed and what it saw.
    ///
    /// Only a successful check has one; a failed check is described by its
    /// errors.
    pub fn summary(&self) -> Option<String> {
        if !self.ok {
            return None;
        }
        Some(match self.diagnostics.len() {
            0 => "Check passed. No errors or warnings found.".to_string(),
            count => format!(
                "Check passed. No errors found. {} warning(s) emitted.",
                count
            ),
        })
    }
}

/// Check the file at `path` and write the result.
///
/// Diagnostics go to stderr and the closing summary to stdout, so a caller can
/// keep the two apart.
pub fn run(path: &Path, format: Format, verify_mir: bool, color_mode: ColorMode) -> Outcome {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read {}: {}", path.display(), error);
            return Outcome::Failed;
        }
    };

    let report = check(path, &source, verify_mir);

    if format == Format::Json {
        println!("{}", report.to_json());
    } else {
        for rendered in report.rendered_diagnostics(&source, color_mode) {
            eprintln!("{}", rendered);
        }
        if let Some(summary) = report.summary() {
            println!("{}", summary);
        }
    }

    if report.ok {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    }
}
