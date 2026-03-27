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
    let colors = ColorScheme::detect();

    let level_color = colors.severity_color(diag.severity);

    let level = diag.severity.as_str();
    let mut output = String::new();

    // Use source override if the diagnostic comes from an imported file.
    let (effective_source, file_label) = match &diag.source_override {
        Some((path, src)) => (src.as_str(), Some(path.as_str())),
        None => (source, None),
    };

    // Header: error[E0001]: Title
    output.push_str(colors.bold);
    output.push_str(level_color);
    output.push_str(level);
    if let Some(code) = diag.code {
        output.push('[');
        output.push_str(code);
        output.push(']');
    }
    output.push_str(": ");
    output.push_str(colors.reset);
    output.push_str(&diag.title);
    output.push('\n');

    // If we have a span that falls within the source, show source context.
    // Spans from stdlib modules may point outside the user's source string;
    // treat those as spanless to avoid panicking in find_line_info.
    let effective_span = diag.span.filter(|s| s.start < effective_source.len());

    if let Some(ref span) = effective_span {
        let (line_num, col_num, line_str) = find_line_info(effective_source, span.start);
        let len = if span.end > span.start {
            span.end - span.start
        } else {
            1
        };

        let gutter_width = line_num.to_string().len();
        let padding = " ".repeat(col_num.saturating_sub(1));
        let underline = "^".repeat(len);

        // Location: --> file:line:col (or just line:col for the main file)
        if let Some(path) = file_label {
            output.push_str(&format!(
                "{}-->{} {}:{}:{}\n",
                colors.blue, colors.reset, path, line_num, col_num
            ));
        } else {
            output.push_str(&format!(
                "{}-->{} line {}:{}\n",
                colors.blue, colors.reset, line_num, col_num
            ));
        }

        // Empty line with pipe
        output.push_str(&format!(
            "{} {} |{}\n",
            " ".repeat(gutter_width),
            colors.blue,
            colors.reset
        ));

        // Code line
        output.push_str(&format!(
            "{} {} |{} {}\n",
            colors.blue, line_num, colors.reset, line_str
        ));

        // Underline with message if different from title
        output.push_str(&format!(
            "{} {} |{} {}{}{}{}",
            " ".repeat(gutter_width),
            colors.blue,
            colors.reset,
            padding,
            colors.bold,
            level_color,
            underline
        ));

        // Inline message if different from title
        if diag.message != diag.title {
            output.push(' ');
            output.push_str(&diag.message);
        }

        output.push_str(colors.reset);
        output.push('\n');

        // Help message on its own line
        if let Some(ref h) = diag.help {
            output.push_str(&format!(
                "{} {} |{}\n",
                " ".repeat(gutter_width),
                colors.blue,
                colors.reset
            ));
            output.push_str(&format!("  {}= help:{} {}\n", colors.cyan, colors.reset, h));
        }

        // Closing empty line with pipe
        output.push_str(&format!(
            "{} {} |{}\n",
            " ".repeat(gutter_width),
            colors.blue,
            colors.reset
        ));
    } else {
        // No span - just show the message if different from title
        if diag.message != diag.title {
            output.push_str(&format!("  = {}\n", diag.message));
        }
        if let Some(ref h) = diag.help {
            output.push_str(&format!("  = help: {}\n", h));
        }
    }

    // Notes
    for note in &diag.notes {
        output.push_str(&format!(
            "  {}= note:{} {}\n",
            colors.cyan, colors.reset, note
        ));
    }

    output
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

    // Threshold for suggestion: roughly 40% of the word length or max 3 edits
    let threshold = std::cmp::max(3, target.len() / 2);
    if min_distance <= threshold {
        best_candidate
    } else {
        None
    }
}
