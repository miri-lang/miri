// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Recovers the comments that sit above declarations.
//!
//! The lexer classifies comments as non-terminals and drops them, so they never
//! reach the AST and the formatter cannot render them. An outline still needs
//! the one line that says what a declaration is for, so the comments are read
//! back from the source text and matched to declarations by position.
//!
//! A comment belongs to the declaration below it when nothing but a single line
//! break separates them. A blank line in between means the comment stands on its
//! own and is left unattached.

use crate::lexer::Lexer;

/// A run of comment lines with nothing but whitespace between them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommentBlock {
    /// The first line of the run, stripped of its marker and surrounding space.
    summary: String,
    /// Byte offset one past the run's last character.
    end: usize,
}

/// The comments in one source file, ready to be matched against declarations.
#[derive(Debug, Clone, Default)]
pub struct DocComments {
    blocks: Vec<CommentBlock>,
}

impl DocComments {
    /// Read every comment in `source`.
    ///
    /// The lexer discards comments rather than emitting them, so they survive
    /// only as the gaps between the tokens it does emit. A gap holds nothing but
    /// whitespace and comments — a string's contents are inside a token, never
    /// in a gap — so the comments can be lifted straight out of it.
    ///
    /// A source that does not lex yields whatever was collected before the bad
    /// token: a comment is a convenience here, never a reason to fail a read.
    pub fn harvest(source: &str) -> Self {
        let mut blocks: Vec<CommentBlock> = Vec::new();
        let mut cursor = 0;

        for token in Lexer::new(source) {
            let Ok((_, span)) = token else {
                break;
            };
            if span.start > cursor {
                collect_from_gap(source, cursor, span.start, &mut blocks);
            }
            cursor = cursor.max(span.end);
        }
        if cursor < source.len() {
            collect_from_gap(source, cursor, source.len(), &mut blocks);
        }

        Self { blocks }
    }

    /// The first line of the comment attached to the declaration at `start`,
    /// or `None` when no comment sits directly above it.
    ///
    /// `start` may point anywhere on the declaration's first line — a statement
    /// carries the span of its name, not of the keyword that opens it — so the
    /// search runs from the start of that line rather than from `start` itself.
    pub fn summary_before(&self, source: &str, start: usize) -> Option<&str> {
        let boundary = declaration_start(source, start);
        let block = self
            .blocks
            .iter()
            .rev()
            .find(|block| block.end <= boundary)?;
        if !is_adjacent(source, block.end, boundary) {
            return None;
        }
        Some(&block.summary)
    }
}

/// Lift the comments out of one gap between tokens.
fn collect_from_gap(source: &str, from: usize, to: usize, blocks: &mut Vec<CommentBlock>) {
    // The offsets come from the lexer rather than from this module, so they are
    // checked before they are used to slice: a comment is a convenience, never
    // a reason to bring the process down.
    let Some(gap) = source.get(from..to) else {
        return;
    };
    let mut offset = from;
    for line in gap.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            offset += line.len();
            continue;
        }
        let start = offset + (line.len() - line.trim_start().len());
        let end = offset + line.trim_end().len();
        let summary = summarize(trimmed);
        match blocks.last_mut() {
            // A comment on the very next line continues the run above it.
            Some(previous) if is_adjacent(source, previous.end, start) => previous.end = end,
            _ => blocks.push(CommentBlock { summary, end }),
        }
        offset += line.len();
    }
}

/// The offset a declaration visually begins at.
///
/// A declaration begins at the start of its own line, and any attribute lines
/// above it belong to it too, so a comment written above `@test` still reaches
/// the function that `@test` marks.
fn declaration_start(source: &str, start: usize) -> usize {
    let mut boundary = line_start(source, start.min(source.len()));
    while boundary > 0 {
        let previous = line_start(source, boundary - 1);
        if !source[previous..boundary].trim_start().starts_with('@') {
            break;
        }
        boundary = previous;
    }
    boundary
}

/// The offset the line holding `offset` begins at.
fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |index| index + 1)
}

/// Whether `from..to` holds at most one line break and no other content.
///
/// One break means the comment is on the line directly above. Two means a blank
/// line separates them, and the comment is not attached.
fn is_adjacent(source: &str, from: usize, to: usize) -> bool {
    if to < from {
        return false;
    }
    let Some(between) = source.get(from..to) else {
        return false;
    };
    if !between.chars().all(char::is_whitespace) {
        return false;
    }
    between.matches('\n').count() <= 1
}

/// The first line of a comment, without its marker or surrounding space.
fn summarize(text: &str) -> String {
    let body = text
        .strip_prefix("//")
        .or_else(|| {
            text.strip_prefix("/*")
                .map(|rest| rest.strip_suffix("*/").unwrap_or(rest))
        })
        .unwrap_or(text);
    body.lines()
        .map(|line| line.trim().trim_start_matches(['/', '*']).trim())
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_is_the_line_above_a_declaration() {
        let source = "// Greets the user.\nfn main()\n    println(\"hi\")\n";
        let comments = DocComments::harvest(source);
        let start = source.find("fn main").expect("the fixture declares main");
        assert_eq!(
            comments.summary_before(source, start),
            Some("Greets the user.")
        );
    }

    #[test]
    fn test_a_blank_line_detaches_the_comment() {
        let source = "// Unrelated note.\n\nfn main()\n    println(\"hi\")\n";
        let comments = DocComments::harvest(source);
        let start = source.find("fn main").expect("the fixture declares main");
        assert_eq!(comments.summary_before(source, start), None);
    }

    #[test]
    fn test_a_run_of_lines_summarizes_to_its_first() {
        let source = "// What it does.\n// More detail.\nfn main()\n    println(\"hi\")\n";
        let comments = DocComments::harvest(source);
        let start = source.find("fn main").expect("the fixture declares main");
        assert_eq!(
            comments.summary_before(source, start),
            Some("What it does.")
        );
    }

    #[test]
    fn test_a_declaration_with_no_comment_has_no_summary() {
        let source = "fn main()\n    println(\"hi\")\n";
        let comments = DocComments::harvest(source);
        let start = source.find("fn main").expect("the fixture declares main");
        assert_eq!(comments.summary_before(source, start), None);
    }

    #[test]
    fn test_the_search_starts_at_the_declaration_line_not_its_name() {
        // A statement carries the span of its name, so the offset a caller has
        // points past the keyword that opens the declaration.
        let source = "// Adds them up.\nfn total(v int) int\n    return v\n";
        let comments = DocComments::harvest(source);
        let name = source
            .find("total")
            .expect("the fixture names the function");
        assert_eq!(comments.summary_before(source, name), Some("Adds them up."));
    }

    #[test]
    fn test_an_attribute_line_does_not_detach_the_comment() {
        let source = "// Checks the sum.\n@test\nfn total_is_right()\n    return\n";
        let comments = DocComments::harvest(source);
        let name = source
            .find("total_is_right")
            .expect("the fixture names the function");
        assert_eq!(
            comments.summary_before(source, name),
            Some("Checks the sum.")
        );
    }

    #[test]
    fn test_a_block_comment_summarizes_to_its_first_line() {
        let source = "/* Greets the user.\n   Twice. */\nfn main()\n    println(\"hi\")\n";
        let comments = DocComments::harvest(source);
        let start = source.find("fn main").expect("the fixture declares main");
        assert_eq!(
            comments.summary_before(source, start),
            Some("Greets the user.")
        );
    }
}
