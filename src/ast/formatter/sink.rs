// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Accumulates rendered text while recording where each declaration landed.
//!
//! Rendering and span recording happen in the same pass. A caller marks the
//! offset before it renders a declaration and records the span once the
//! declaration is fully written, so a span always delimits exactly the bytes
//! that declaration produced. Nothing re-scans the finished text, which is what
//! keeps the offsets true no matter how a declaration is nested.

use serde::{Deserialize, Serialize};

use crate::ast::statement::WrittenModifiers;
use crate::error::syntax::Span;

/// One indentation level of rendered output.
const INDENT_UNIT: &str = "    ";

/// The byte range one declaration occupies in rendered text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedSpan {
    /// Byte offset where the declaration starts.
    pub start: usize,
    /// Byte offset one past the declaration's last byte.
    pub end: usize,
    /// What kind of declaration this is, such as `function` or `class`.
    pub kind: String,
    /// The declared name, when the declaration has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Where a declaration's rendering began.
///
/// Returned by [`Sink::mark`] and consumed by [`Sink::record`], so a span
/// cannot be recorded without a matching mark.
#[derive(Debug, Clone, Copy)]
pub struct Mark(usize);

/// Collects rendered text and the spans of the declarations within it.
#[derive(Debug, Clone, Default)]
pub struct Sink {
    text: String,
    spans: Vec<RecordedSpan>,
    comments: bool,
    modifiers: WrittenModifiers,
    source: Option<String>,
}

impl Sink {
    /// An empty sink that renders code only.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty sink that also renders the comments a statement carries, and
    /// that can consult `source` for spellings the tree does not keep.
    ///
    /// Only whole-program rendering asks for either. A single declaration is
    /// rendered without comments because that text is what an edit anchor is
    /// matched against, and an anchor that could match inside a comment would
    /// let an edit land there.
    pub fn with_comments(source: &str) -> Self {
        Self {
            comments: true,
            source: Some(source.to_string()),
            ..Self::default()
        }
    }

    /// An empty sink that renders code only, and that can consult `source` for
    /// spellings the tree does not keep.
    ///
    /// The alignment between a canonical rendering and the file it came from
    /// is token by token, so the rendering has to spell a number the way the
    /// file spells it: a declaration holding `0xFF` cannot be aligned against
    /// a rendering that says `255`, and so could not be patched at all.
    pub fn reading(source: &str) -> Self {
        Self {
            source: Some(source.to_string()),
            ..Self::default()
        }
    }

    /// The source text `span` covers, when this sink was given the text the
    /// tree was parsed from and the span lies inside it.
    ///
    /// A tree built without source — by the AST factory, or by a pass that
    /// rewrote it — has no spelling to give back, and every caller renders
    /// from the tree instead.
    pub fn source_text(&self, span: Span) -> Option<&str> {
        let source = self.source.as_deref()?;
        if span.start >= span.end || span.end > source.len() {
            return None;
        }
        if !source.is_char_boundary(span.start) || !source.is_char_boundary(span.end) {
            return None;
        }
        Some(&source[span.start..span.end])
    }

    /// Whether this sink renders comments.
    pub fn renders_comments(&self) -> bool {
        self.comments
    }

    /// The meaning-free modifiers the declaration being rendered was written
    /// with.
    ///
    /// The renderer that writes a declaration's modifiers sits several calls
    /// below the one holding this fact, so it travels here rather than through
    /// every header in between.
    pub fn written_modifiers(&self) -> WrittenModifiers {
        self.modifiers
    }

    /// Render `body` with `modifiers` as the answer to
    /// [`Sink::written_modifiers`].
    ///
    /// The previous answer is restored afterwards, so a nested declaration
    /// cannot leave its own modifiers behind for the one that contains it.
    pub fn with_written_modifiers<R>(
        &mut self,
        modifiers: WrittenModifiers,
        render: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.modifiers, modifiers);
        let result = render(self);
        self.modifiers = previous;
        result
    }

    /// Whether the cursor sits at the start of a line, with only indentation
    /// written since the last break.
    ///
    /// A statement is rendered inline in the single-line `:` form, where a
    /// comment cannot be written above it without breaking the line in two.
    pub fn at_line_start(&self) -> bool {
        match self.text.rfind('\n') {
            Some(break_at) => self.text[break_at + 1..].chars().all(|c| c == ' '),
            None => self.text.chars().all(|c| c == ' '),
        }
    }

    /// Append text.
    pub fn emit(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// Append a line break followed by `level` indentation units.
    pub fn emit_line(&mut self, level: usize) {
        self.text.push('\n');
        self.emit_indent(level);
    }

    /// Append `level` indentation units.
    pub fn emit_indent(&mut self, level: usize) {
        for _ in 0..level {
            self.text.push_str(INDENT_UNIT);
        }
    }

    /// Note the current offset so a span can be recorded from here.
    pub fn mark(&mut self) -> Mark {
        Mark(self.text.len())
    }

    /// Record a span running from `mark` to the current offset.
    pub fn record(&mut self, mark: Mark, kind: &str, name: Option<String>) {
        self.spans.push(RecordedSpan {
            start: mark.0,
            end: self.text.len(),
            kind: kind.to_string(),
            name,
        });
    }

    /// The text rendered so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether nothing has been rendered yet.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Take the rendered text and the recorded spans.
    pub fn finish(self) -> (String, Vec<RecordedSpan>) {
        (self.text, self.spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_appends_text() {
        let mut sink = Sink::new();
        sink.emit("fn main()");
        assert_eq!(sink.text(), "fn main()");
    }

    #[test]
    fn test_emit_line_breaks_and_indents() {
        let mut sink = Sink::new();
        sink.emit("fn main()");
        sink.emit_line(1);
        sink.emit("println()");
        assert_eq!(sink.text(), "fn main()\n    println()");
    }

    #[test]
    fn test_record_delimits_exactly_what_was_rendered() {
        let mut sink = Sink::new();
        sink.emit("use system.io\n");
        let mark = sink.mark();
        sink.emit("fn main()");
        sink.record(mark, "function", Some("main".to_string()));

        let (text, spans) = sink.finish();
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].start..spans[0].end], "fn main()");
        assert_eq!(spans[0].name.as_deref(), Some("main"));
    }
}
