// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Diagnostic formatting utilities.
//!
//! This module provides functions for formatting error and warning messages
//! with source context, underlining, and helpful suggestions.

use crate::error::diagnostic::{Diagnostic, Severity};
use crate::error::syntax::find_line_info;
use crate::error::syntax::Span;

/// Format a diagnostic using the full Diagnostic struct.
///
/// This is the primary formatting function that displays:
/// - Error/warning level with color
/// - Optional error code
/// - Title and message
/// - Source code context with underline
/// - Help text and notes
pub fn format_diagnostic_full(source: &str, diag: &Diagnostic) -> String {
    // Colors
    let color_reset = "\x1b[0m";
    let color_bold = "\x1b[1m";
    let color_red = "\x1b[31m";
    let color_yellow = "\x1b[33m";
    let color_blue = "\x1b[34m";
    let color_cyan = "\x1b[36m";

    let level_color = match diag.severity {
        Severity::Error => color_red,
        Severity::Warning => color_yellow,
        Severity::Note => color_cyan,
    };

    let level = diag.severity.as_str();
    let mut output = String::new();

    // Header: error[E0001]: Title
    output.push_str(color_bold);
    output.push_str(level_color);
    output.push_str(level);
    if let Some(code) = diag.code {
        output.push('[');
        output.push_str(code);
        output.push(']');
    }
    output.push_str(": ");
    output.push_str(color_reset);
    output.push_str(&diag.title);
    output.push('\n');

    // If we have a span, show source context
    if let Some(ref span) = diag.span {
        let (line_num, col_num, line_str) = find_line_info(source, span.start);
        let len = if span.end > span.start {
            span.end - span.start
        } else {
            1
        };

        let gutter_width = line_num.to_string().len();
        let padding = " ".repeat(col_num.saturating_sub(1));
        let underline = "^".repeat(len);

        // Location: --> line:col
        output.push_str(&format!(
            "{}-->{} line {}:{}\n",
            color_blue, color_reset, line_num, col_num
        ));

        // Empty line with pipe
        output.push_str(&format!(
            "{} {} |{}\n",
            " ".repeat(gutter_width),
            color_blue,
            color_reset
        ));

        // Code line
        output.push_str(&format!(
            "{} {} |{} {}\n",
            color_blue, line_num, color_reset, line_str
        ));

        // Underline with message if different from title
        output.push_str(&format!(
            "{} {} |{} {}{}{}{}",
            " ".repeat(gutter_width),
            color_blue,
            color_reset,
            padding,
            color_bold,
            level_color,
            underline
        ));

        // Inline message if different from title
        if diag.message != diag.title {
            output.push(' ');
            output.push_str(&diag.message);
        }

        // Help message inline with underline
        if let Some(ref h) = diag.help {
            output.push_str(&format!(" help: {}", h));
        }
        output.push_str(color_reset);
        output.push('\n');

        // Closing empty line with pipe
        output.push_str(&format!(
            "{} {} |{}\n",
            " ".repeat(gutter_width),
            color_blue,
            color_reset
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
            color_cyan, color_reset, note
        ));
    }

    output
}

/// Legacy format function for backward compatibility.
///
/// This function is kept for existing code that hasn't migrated to Diagnostic yet.
/// New code should use `Diagnostic::format()` or `format_diagnostic_full()`.
pub fn format_diagnostic(
    source: &str,
    span: &Span,
    message: &str,
    level: &str,
    help: Option<&str>,
) -> String {
    let (line_num, col_num, line_str) = find_line_info(source, span.start);
    let len = if span.end > span.start {
        span.end - span.start
    } else {
        1
    };

    // Colors
    let color_reset = "\x1b[0m";
    let color_bold = "\x1b[1m";
    let color_red = "\x1b[31m";
    let color_yellow = "\x1b[33m";
    let color_blue = "\x1b[34m";

    let level_color = if level == "error" {
        color_red
    } else {
        color_yellow
    };

    let gutter_width = line_num.to_string().len();
    let padding = " ".repeat(col_num.saturating_sub(1));
    let underline = "^".repeat(len);

    let mut output = String::new();

    // Header: error: message
    output.push_str(&format!(
        "{}{}{}: {}{}\n",
        color_bold, level_color, level, color_reset, message
    ));

    // Location: --> line:col
    output.push_str(&format!(
        "{}-->{} line {}:{}\n",
        color_blue, color_reset, line_num, col_num
    ));

    // Empty line with pipe
    output.push_str(&format!(
        "{} {} |{}\n",
        " ".repeat(gutter_width),
        color_blue,
        color_reset
    ));

    // Code line
    output.push_str(&format!(
        "{} {} |{} {}\n",
        color_blue, line_num, color_reset, line_str
    ));

    // Underline
    output.push_str(&format!(
        "{} {} |{} {}{}{}{}",
        " ".repeat(gutter_width),
        color_blue,
        color_reset,
        padding,
        color_bold,
        level_color,
        underline
    ));

    // Help message inline with underline if short, or on next line
    if let Some(h) = help {
        output.push_str(&format!(" help: {}", h));
    }
    output.push_str(color_reset);
    output.push('\n');

    // Closing empty line with pipe
    output.push_str(&format!(
        "{} {} |{}\n",
        " ".repeat(gutter_width),
        color_blue,
        color_reset
    ));

    output
}

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
