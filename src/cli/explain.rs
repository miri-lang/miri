// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Rendering for the `explain` command.
//!
//! Colour and terminal presentation belong here rather than in
//! `crate::diagnostics`, which is the inner layer and must not depend on the CLI.
//! This module reads an `Explanation` and turns it into either human-readable
//! text or the JSON envelope shared with the other commands.

use std::str::FromStr;

use crate::cli::{sanitize_for_terminal, serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic};
use crate::diagnostics::{DiagnosticCode, Explanation, Severity};
use crate::error::format::ColorScheme;

/// What the caller should do once the command has written its output.
pub enum Outcome {
    /// The code was found and explained.
    Explained,
    /// The argument was not a code in the registry.
    UnknownCode,
}

/// The result of an explain: either an explanation or an unknown code error.
pub enum ExplainResult {
    /// The code was found and explained.
    Found(Explanation),
    /// The argument is not a code in the registry.
    UnknownCode,
}

/// Explain one diagnostic code without side effects.
///
/// Returns either the explanation for a valid code or an error for an unknown code,
/// with no printing or process exit.
pub fn explain_core(code: &str) -> ExplainResult {
    match DiagnosticCode::from_str(code) {
        Ok(found) => ExplainResult::Found(found.explanation()),
        Err(_) => ExplainResult::UnknownCode,
    }
}

/// Build the envelope that answers a request to explain `code`.
///
/// An unknown code is answered as a diagnostic rather than as a failure of the
/// transport, so a machine consumer reads it the way it reads every other
/// diagnostic — including from the command whose whole subject is codes.
pub fn envelope(code: &str) -> DiagnosticsEnvelope {
    match explain_core(code) {
        ExplainResult::Found(explanation) => {
            DiagnosticsEnvelope::new(JsonCommand::Explain, true, vec![])
                .with_exit_code(0)
                .with_explanation(explanation.to_json())
        }
        ExplainResult::UnknownCode => {
            DiagnosticsEnvelope::new(JsonCommand::Explain, false, vec![unknown_code(code)])
                .with_exit_code(1)
        }
    }
}

/// Serialize an explanation into the envelope shared with the other commands.
fn render_json(explanation: &Explanation) -> String {
    let envelope = DiagnosticsEnvelope::new(JsonCommand::Explain, true, vec![])
        .with_exit_code(0)
        .with_explanation(explanation.to_json());
    serialize_envelope(&envelope)
}

/// Render an explanation as human-readable text.
fn render_pretty(explanation: &Explanation, color_mode: ColorMode) -> String {
    let scheme = ColorScheme::from_choice(color_mode.into());
    let code = explanation.code;
    let accent = match code.severity() {
        Severity::Error => scheme.red,
        Severity::Warning => scheme.yellow,
        Severity::Note => scheme.cyan,
    };

    let mut out = format!(
        "{}{}{}{} {}{}\n{}\n",
        scheme.bold,
        accent,
        code.as_str(),
        scheme.reset,
        code.title(),
        scheme.reset,
        code.severity().as_str(),
    );

    if code.is_reserved() {
        out.push_str(&format!(
            "\n{}This code is retired and is no longer emitted. Its number stays \
             reserved so it is never handed to a different diagnosis.{}\n",
            scheme.blue, scheme.reset
        ));
    }

    out.push_str(&section(&scheme, "Rule", &explanation.rule));
    if let Some(before) = &explanation.example_before {
        out.push_str(&section(&scheme, "Before", before));
    }
    if let Some(after) = &explanation.example_after {
        out.push_str(&section(&scheme, "After", after));
    }
    if let Some(reference) = &explanation.reference {
        // Convert ../reference/path.md to docs/reference/path.md for readability
        let repo_relative_path = if reference.starts_with("../reference/") {
            format!("docs/{}", &reference[3..]) // Skip the "../" prefix
        } else {
            reference.clone()
        };

        let ref_text = if let (Some(title), Some(summary)) =
            (&explanation.reference_title, &explanation.reference_summary)
        {
            format!("{} ({})\n\n{}", title, repo_relative_path, summary)
        } else {
            repo_relative_path
        };
        out.push_str(&section(&scheme, "Reference", &ref_text));
    }
    out
}

/// Render one titled block of the explanation.
fn section(scheme: &ColorScheme, heading: &str, body: &str) -> String {
    format!(
        "\n{}{}{}{}\n{}\n",
        scheme.bold, scheme.blue, heading, scheme.reset, body
    )
}

/// Explain one diagnostic code, writing the result to stdout or stderr.
///
/// An unknown code is itself reported as a diagnostic, so a machine consumer
/// gets a coded failure from the command whose subject is codes.
pub fn run(code: &str, format: Format, color_mode: ColorMode) -> Outcome {
    match explain_core(code) {
        ExplainResult::Found(explanation) => {
            let output = match format {
                Format::Json => render_json(&explanation),
                Format::Pretty => render_pretty(&explanation, color_mode),
            };
            println!("{}", output);
            Outcome::Explained
        }
        ExplainResult::UnknownCode => {
            report_unknown_code(code, format, color_mode);
            Outcome::UnknownCode
        }
    }
}

/// The help text offered alongside an unrecognised code.
const UNKNOWN_CODE_HELP: &str = "codes have the form MER_<AREA>_<NUM>, for example MER_TYP_030. \
                                 Run `miri explain` with a code from the registry.";

/// Describe an argument that is not a code in the registry.
///
/// One definition serves both transports, so the command line and a request
/// over a connection report the same thing.
fn unknown_code(code: &str) -> JsonDiagnostic {
    let unknown = DiagnosticCode::BldUnknownDiagnosticCode;
    JsonDiagnostic {
        severity: unknown.severity().as_str().to_string(),
        code: Some(unknown.as_str().to_string()),
        message: format!("unknown diagnostic code: {}", sanitize_for_terminal(code)),
        path: None,
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: Some(UNKNOWN_CODE_HELP.to_string()),
        fix_safety: None,
        repair: None,
        related: vec![],
        preexisting: None,
    }
}

/// Report an argument that is not a code in the registry.
fn report_unknown_code(code: &str, format: Format, color_mode: ColorMode) {
    let unknown = DiagnosticCode::BldUnknownDiagnosticCode;
    let message = format!("unknown diagnostic code: {}", sanitize_for_terminal(code));

    match format {
        Format::Json => {
            println!("{}", serialize_envelope(&envelope(code)));
        }
        Format::Pretty => {
            let scheme = ColorScheme::from_choice(color_mode.into());
            eprintln!(
                "{}{}error[{}]{}: {}\n  {}",
                scheme.bold,
                scheme.red,
                unknown.as_str(),
                scheme.reset,
                message,
                UNKNOWN_CODE_HELP,
            );
        }
    }
}
