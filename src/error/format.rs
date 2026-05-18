// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic formatting utilities.
//!
//! This module provides functions for formatting error and warning messages
//! with source context, underlining, and helpful suggestions.
//!
//! All color output is gated on TTY detection: when stderr is not a terminal
//! (e.g. piped to a file or another process), ANSI escape codes are omitted
//! entirely so that output remains clean and parseable.

use std::io::IsTerminal;

use crate::error::diagnostic::{Diagnostic, Severity};
use crate::error::syntax::find_line_info;

/// ANSI color scheme that resolves to real escape codes on a TTY
/// or empty strings when output is redirected.
///
/// All diagnostic formatting flows through this struct to ensure
/// consistent color handling across the entire error pipeline.
pub struct ColorScheme {
    /// Reset all attributes.
    pub reset: &'static str,
    /// Bold text.
    pub bold: &'static str,
    /// Red (used for errors).
    pub red: &'static str,
    /// Yellow (used for warnings).
    pub yellow: &'static str,
    /// Blue (used for line-number gutters and location arrows).
    pub blue: &'static str,
    /// Cyan (used for notes).
    pub cyan: &'static str,
}

impl ColorScheme {
    /// Detects whether stderr is a terminal and returns the appropriate scheme.
    pub fn detect() -> Self {
        if std::io::stderr().is_terminal() {
            Self::colored()
        } else {
            Self::plain()
        }
    }

    /// Returns a scheme with real ANSI escape codes.
    pub fn colored() -> Self {
        Self {
            reset: "\x1b[0m",
            bold: "\x1b[1m",
            red: "\x1b[31m",
            yellow: "\x1b[33m",
            blue: "\x1b[34m",
            cyan: "\x1b[36m",
        }
    }

    /// Returns a scheme with empty strings (no color).
    pub fn plain() -> Self {
        Self {
            reset: "",
            bold: "",
            red: "",
            yellow: "",
            blue: "",
            cyan: "",
        }
    }

    /// Returns the color associated with the given severity level.
    pub fn severity_color(&self, severity: Severity) -> &'static str {
        match severity {
            Severity::Error => self.red,
            Severity::Warning => self.yellow,
            Severity::Note => self.cyan,
        }
    }
}

/// Format a diagnostic using the full Diagnostic struct.
///
/// This is the primary formatting function that displays:
/// - Error/warning level with color
/// - Optional error code
/// - Title and message
/// - Source code context with underline
/// - Help text and notes
///
/// Color output is automatically enabled when stderr is a terminal and
/// disabled when output is redirected to a file or pipe.
pub fn format_diagnostic_full(source: &str, diag: &Diagnostic) -> String {
    format_diagnostic(source, diag, None)
}

/// Formats a diagnostic with an optional fallback file path for the main
/// source file.  When `source_path` is `Some`, errors that do *not* carry
/// their own `source_override` (i.e. errors from the entry-point file) will
/// display the given path in the `-->` location line.
pub fn format_diagnostic(source: &str, diag: &Diagnostic, source_path: Option<&str>) -> String {
    let colors = ColorScheme::detect();
    let (effective_source, file_label) = effective_source_and_label(diag, source, source_path);

    let mut output = String::new();
    append_header(&mut output, diag, &colors);

    let span_in_source = diag.span.filter(|s| s.start <= effective_source.len());
    match span_in_source {
        Some(span) => append_span_context(
            &mut output,
            diag,
            effective_source,
            file_label,
            span,
            &colors,
        ),
        None => append_spanless_body(&mut output, diag),
    }

    append_notes(&mut output, diag, &colors);
    output
}

fn effective_source_and_label<'a>(
    diag: &'a Diagnostic,
    main_source: &'a str,
    main_path: Option<&'a str>,
) -> (&'a str, Option<&'a str>) {
    match &diag.source_override {
        Some((path, src)) => (src.as_str(), Some(path.as_str())),
        None => (main_source, main_path),
    }
}

fn append_header(out: &mut String, diag: &Diagnostic, colors: &ColorScheme) {
    let level_color = colors.severity_color(diag.severity);
    out.push_str(colors.bold);
    out.push_str(level_color);
    out.push_str(diag.severity.as_str());
    if let Some(code) = diag.code {
        out.push('[');
        out.push_str(code);
        out.push(']');
    }
    out.push_str(": ");
    out.push_str(colors.reset);
    out.push_str(&diag.title);
    out.push('\n');
}

fn append_span_context(
    out: &mut String,
    diag: &Diagnostic,
    source: &str,
    file_label: Option<&str>,
    span: crate::error::syntax::Span,
    colors: &ColorScheme,
) {
    let level_color = colors.severity_color(diag.severity);
    let (line_num, col_num, line_str) = find_line_info(source, span.start);
    let len = span.end.saturating_sub(span.start).max(1);
    let gutter_width = line_num.to_string().len();
    let gutter = " ".repeat(gutter_width);
    let padding = " ".repeat(col_num.saturating_sub(1));
    let underline = "^".repeat(len);

    if let Some(path) = file_label {
        out.push_str(&format!(
            "{}-->{} {}:{}:{}\n",
            colors.blue, colors.reset, path, line_num, col_num
        ));
    } else {
        out.push_str(&format!(
            "{}-->{} line {}:{}\n",
            colors.blue, colors.reset, line_num, col_num
        ));
    }

    out.push_str(&format!("{} {} |{}\n", gutter, colors.blue, colors.reset));
    out.push_str(&format!(
        "{} {} |{} {}\n",
        colors.blue, line_num, colors.reset, line_str
    ));
    out.push_str(&format!(
        "{} {} |{} {}{}{}{}",
        gutter, colors.blue, colors.reset, padding, colors.bold, level_color, underline
    ));
    if diag.message != diag.title {
        out.push(' ');
        out.push_str(&diag.message);
    }
    out.push_str(colors.reset);
    out.push('\n');

    if let Some(ref help) = diag.help {
        out.push_str(&format!("{} {} |{}\n", gutter, colors.blue, colors.reset));
        out.push_str(&format!(
            "  {}= help:{} {}\n",
            colors.cyan, colors.reset, help
        ));
    }
    out.push_str(&format!("{} {} |{}\n", gutter, colors.blue, colors.reset));
}

fn append_spanless_body(out: &mut String, diag: &Diagnostic) {
    if diag.message != diag.title {
        out.push_str(&format!("  = {}\n", diag.message));
    }
    if let Some(ref help) = diag.help {
        out.push_str(&format!("  = help: {}\n", help));
    }
}

fn append_notes(out: &mut String, diag: &Diagnostic, colors: &ColorScheme) {
    for note in &diag.notes {
        out.push_str(&format!(
            "  {}= note:{} {}\n",
            colors.cyan, colors.reset, note
        ));
    }
}

/// Computes the Levenshtein edit distance between two strings.
pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate() {
        *cell = j;
    }

    for (i, c1) in s1.chars().enumerate() {
        for (j, c2) in s2.chars().enumerate() {
            let cost = if c1 == c2 { 0 } else { 1 };
            matrix[i + 1][j + 1] = std::cmp::min(
                std::cmp::min(matrix[i][j + 1] + 1, matrix[i + 1][j] + 1),
                matrix[i][j] + cost,
            );
        }
    }

    matrix[len1][len2]
}

/// Finds the closest match to `target` among `candidates` using edit distance.
/// Returns `None` if no candidate is within a reasonable threshold.
pub fn find_best_match<S: AsRef<str>>(target: &str, candidates: &[S]) -> Option<String> {
    let mut best_candidate = None;
    let mut min_distance = usize::MAX;

    for candidate in candidates {
        let candidate_str = candidate.as_ref();
        let distance = levenshtein_distance(target, candidate_str);
        if distance < min_distance {
            min_distance = distance;
            best_candidate = Some(candidate_str.to_string());
        }
    }

    // Threshold for suggestion: roughly 33% of the word length, minimum 2 edits.
    // Tighter than the old max(3, len/2) to avoid suggesting unrelated names
    // for short identifiers (e.g. "User" no longer matches "Err").
    let threshold = std::cmp::max(2, target.len() / 3);
    if min_distance <= threshold {
        best_candidate
    } else {
        None
    }
}
