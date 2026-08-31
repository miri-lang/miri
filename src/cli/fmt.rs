// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri fmt` command: rewrite a file to its canonical form.
//!
//! The command reads a source file, parses it, renders it to canonical form,
//! and then re-parses and re-renders to verify idempotence. If the file is
//! not already canonical, it is rewritten (unless `--check` is given).
//!
//! The command is split so that the work and the writing are separate. [`fmt`]
//! rewrites the text and returns what it found; it prints nothing and can
//! therefore be called by a long-lived server as readily as by the command line.
//! [`run`] adds the writing and is what the binary calls.

use std::path::Path;

use crate::ast::formatter;
use crate::cli::{serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
use crate::error::diagnostic::{Diagnostic, DiagnosticBuilder, Reportable};
use crate::error::format::format_diagnostic_with_color;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The file was already canonical, or has been rewritten to be.
    Succeeded,
    /// The file could not be parsed, could not be rendered stably, or — under
    /// [`Mode::Check`] — is not canonical.
    Failed,
}

/// What the command is being asked to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Rewrite the file when it is not canonical.
    Write,
    /// Report whether the file is canonical, writing nothing.
    Check,
}

/// What a format found.
///
/// The diagnostics are kept in their compiler form alongside the envelope
/// because the human-readable rendering needs the span and source context that
/// the JSON projection flattens away.
pub struct FmtReport {
    /// The envelope, ready to serialize for a machine consumer.
    pub envelope: DiagnosticsEnvelope,
    /// Whether the command succeeded. Under [`Mode::Check`] a file that is not
    /// canonical is a failure, because that is the question `--check` asks.
    pub ok: bool,
    /// The diagnostics as the compiler reported them, if any.
    diagnostics: Vec<Diagnostic>,
    /// The path the diagnostics were reported against.
    source_path: Option<String>,
    /// Whether the canonical text differs from what the file holds.
    pub changed: bool,
    /// What the file should hold, once it is known to render stably.
    pub canonical_text: Option<String>,
    /// What the command was asked to do, which decides how it reports.
    mode: Mode,
}

/// Render `source` to its canonical form and check that doing so is stable.
///
/// Nothing here writes to a stream, to the file, or ends the process, so the
/// same call serves the command line and a request over a long-lived
/// connection. [`run`] owns the writing.
pub fn fmt(path: &Path, source: &str, mode: Mode) -> FmtReport {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let source_path = absolute.display().to_string();

    let program = match parse(source) {
        Ok(program) => program,
        Err(error) => return refusal(vec![*error], source, source_path, mode),
    };

    let canonical = formatter::program(&program).text;

    // Rendering must be a fixed point: text that renders differently on a
    // second pass would churn the file on every run, so it is refused rather
    // than written.
    match parse(&canonical) {
        Ok(reparsed) => {
            let again = formatter::program(&reparsed).text;
            if canonical != again {
                return refusal(vec![not_idempotent()], &canonical, source_path, mode);
            }
        }
        Err(error) => return refusal(vec![*error], &canonical, source_path, mode),
    }

    // Rendering comes from the tree, so a comment the tree does not carry is a
    // comment the rewrite would delete. Refusing beats writing a file the
    // author has to notice is missing something.
    if let Some(lost) = comment_lost(source, &canonical) {
        return refusal(vec![would_lose(&lost)], source, source_path, mode);
    }

    let changed = canonical != source;
    // `--check` asks whether the file is already canonical, so a file that
    // would be rewritten is the answer "no", not a success.
    let ok = mode == Mode::Write || !changed;
    let envelope = DiagnosticsEnvelope::new(JsonCommand::Fmt, ok, vec![]).with_exit_code(if ok {
        0
    } else {
        1
    });

    FmtReport {
        envelope,
        ok,
        diagnostics: vec![],
        source_path: Some(source_path),
        changed,
        canonical_text: Some(canonical),
        mode,
    }
}

/// Parse exactly what was written: no normalization, no script-mode wrapping.
fn parse(source: &str) -> Result<crate::ast::Program, Box<Diagnostic>> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    parser
        .parse()
        .map_err(|error| Box::new(error.to_diagnostic()))
}

/// The first comment in `source` that `canonical` does not carry, if any.
fn comment_lost(source: &str, canonical: &str) -> Option<String> {
    let mut rendered = comments_in(canonical);
    for comment in comments_in(source) {
        match rendered.iter().position(|kept| *kept == comment) {
            Some(index) => {
                rendered.remove(index);
            }
            None => return Some(comment),
        }
    }
    None
}

/// Every comment `text` holds, in the order the lexer meets them.
fn comments_in(text: &str) -> Vec<String> {
    let mut lexer = Lexer::new(text);
    for token in lexer.by_ref() {
        if token.is_err() {
            break;
        }
    }
    let mut found = lexer.take_leading_comments();
    found.extend(lexer.take_trailing_comments());
    found.into_iter().map(|comment| comment.text).collect()
}

/// The refusal raised when rendering would drop a comment.
fn would_lose(comment: &str) -> Diagnostic {
    DiagnosticBuilder::error(
        crate::diagnostics::DiagnosticCode::BldFormatWouldLoseContent
            .title()
            .to_string(),
    )
    .code(crate::diagnostics::DiagnosticCode::BldFormatWouldLoseContent.as_str())
    .message(format!("formatting would drop the comment `{}`", comment))
    .help(
        "the canonical rendering is derived from the parsed program, and this comment sits \
         where the program does not carry it; the file was left as it was",
    )
    .build()
}

/// The refusal raised when a second render disagrees with the first.
fn not_idempotent() -> Diagnostic {
    DiagnosticBuilder::error(
        crate::diagnostics::DiagnosticCode::BldFormatterNotIdempotent
            .title()
            .to_string(),
    )
    .code(crate::diagnostics::DiagnosticCode::BldFormatterNotIdempotent.as_str())
    .message("the canonical text does not render to itself".to_string())
    .help(
        "rendering the file produced text that renders differently again, so writing it \
         would change the file on every run; the file was left as it was",
    )
    .build()
}

/// A report carrying `diagnostics` and nothing else, with the file untouched.
///
/// `against` is the text the diagnostics point into, which is the file for a
/// parse failure and the rendered text for a refusal raised about the render.
fn refusal(
    diagnostics: Vec<Diagnostic>,
    against: &str,
    source_path: String,
    mode: Mode,
) -> FmtReport {
    let envelope = DiagnosticsEnvelope::new(
        JsonCommand::Fmt,
        false,
        diagnostics
            .iter()
            .map(|diagnostic| {
                crate::error::diagnostic::to_json(diagnostic, against, Some(&source_path))
            })
            .collect(),
    )
    .with_exit_code(1);

    FmtReport {
        envelope,
        ok: false,
        diagnostics,
        source_path: Some(source_path),
        changed: false,
        canonical_text: None,
        mode,
    }
}

impl FmtReport {
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

    /// The closing line that says what the command did.
    pub fn summary(&self) -> Option<String> {
        if !self.ok {
            return None;
        }
        Some(match (self.mode, self.changed) {
            (Mode::Check, _) => "The file is in canonical form.".to_string(),
            (Mode::Write, true) => "Rewritten to canonical form.".to_string(),
            (Mode::Write, false) => "Already in canonical form.".to_string(),
        })
    }
}

/// Format the file at `path` and write the result, or check if already canonical.
///
/// With `check=true`, the file is not written; instead, a non-zero exit code
/// is returned if the file is not already canonical.
pub fn run(path: &Path, check: bool, format: Format, color_mode: ColorMode) -> Outcome {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: could not read {}: {}", path.display(), error);
            return Outcome::Failed;
        }
    };

    let mode = if check { Mode::Check } else { Mode::Write };
    let report = fmt(path, &source, mode);

    if format == Format::Json {
        println!("{}", report.to_json());
    } else {
        for rendered in report.rendered_diagnostics(&source, color_mode) {
            eprintln!("{}", rendered);
        }
        if let Some(summary) = report.summary() {
            println!("{}", summary);
        }
        if mode == Mode::Check && report.changed {
            eprintln!(
                "{} is not in canonical form; run `miri fmt` to rewrite it.",
                path.display()
            );
        }
    }

    if !report.ok {
        return Outcome::Failed;
    }

    if mode == Mode::Write && report.changed {
        if let Some(canonical) = report.canonical_text {
            if let Err(error) = std::fs::write(path, canonical) {
                eprintln!("error: could not write {}: {}", path.display(), error);
                return Outcome::Failed;
            }
        }
    }

    Outcome::Succeeded
}
