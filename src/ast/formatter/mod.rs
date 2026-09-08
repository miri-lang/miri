// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders a parsed program back to canonical Miri source.
//!
//! The rendering is canonical: it is derived from the AST, so blank lines and
//! the author's spacing are normalized away and one program shape always
//! produces one text. That is what lets a tool read a declaration here and
//! anchor an edit against the same bytes later.
//!
//! Canonical means layout, not content. Beyond spacing, exactly these are
//! rewritten, and nothing else is:
//!
//! - a grouping parenthesis that changes no binding is dropped;
//! - a collection type is written in the sugar the language prefers, so
//!   `List<T>` becomes `[T]`;
//! - a body written after a colon becomes an indented block, and an `if` in
//!   expression position becomes the conditional-expression spelling;
//! - a string is written between double quotes.
//!
//! Everything else the author wrote comes back: every modifier, every comment,
//! and the spelling of every number, which the tree does not keep and the
//! renderer therefore reads from the source it was parsed from. `miri fmt`
//! refuses to write a file when a word or a comment did not survive, so a
//! renderer that starts dropping one reports it rather than deleting it.
//!
//! Comments are the one thing rendered two ways. [`program`] keeps them, so
//! rewriting a file to its canonical text does not cost the author their
//! notes. [`declaration`] drops them, because that text is what an edit anchor
//! is matched against: an anchor able to match inside a comment could quietly
//! edit the comment instead of the code.
//!
//! Rendering records spans as it goes, so every declaration in the output comes
//! with the byte range it occupies in that output. The spans index the rendered
//! text, not the file the AST was parsed from.

pub mod expression;
pub mod helpers;
pub mod pattern;
pub mod signature;
pub mod sink;
pub mod statement;
pub mod types;

use crate::ast::{Program, Statement};

pub use signature::{signature, Signature};
pub use sink::RecordedSpan;
use sink::Sink;

/// Canonical text together with the spans of the declarations inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The rendered source.
    pub text: String,
    /// Where each declaration landed in [`Rendered::text`].
    pub spans: Vec<RecordedSpan>,
}

/// Render a whole program, comments included.
///
/// `source` is the text `program` was parsed from. Rendering consults it for
/// the spellings the tree does not keep — a number written `0xFF` rather than
/// `255` — and falls back to rendering from the tree wherever the span does not
/// answer, so a tree that came from somewhere else still renders.
pub fn program(program: &Program, source: &str) -> Rendered {
    let mut sink = Sink::with_comments(source);
    for (index, entry) in program.body.iter().enumerate() {
        if index > 0 {
            sink.emit("\n");
        }
        statement::statement(&mut sink, entry, 0);
    }
    if !sink.is_empty() {
        sink.emit("\n");
    }
    finish(sink)
}

/// Render a single expression to canonical text.
pub fn expression_text(node: &crate::ast::expression::Expression) -> String {
    let mut sink = Sink::new();
    expression::expression(&mut sink, node, 0);
    sink.text().to_string()
}

/// Render a single statement as if it stood alone at the top level.
///
/// Rendered from the tree alone, so two declarations that mean the same thing
/// render to the same text whatever the files they came from spelled. That is
/// what makes this the right rendering to compare two versions of a
/// declaration with.
pub fn declaration(node: &Statement) -> Rendered {
    render_declaration(Sink::new(), node)
}

/// Render a single statement, reading the spellings the tree does not keep
/// from the `source` it was parsed from.
///
/// This is the rendering to align against a file: alignment is token by token,
/// so a declaration holding a number written `0xFF` has to render it that way
/// or it cannot be aligned, and cannot then be patched.
pub fn declaration_from_source(node: &Statement, source: &str) -> Rendered {
    render_declaration(Sink::reading(source), node)
}

fn render_declaration(mut sink: Sink, node: &Statement) -> Rendered {
    statement::statement(&mut sink, node, 0);
    if !sink.is_empty() {
        sink.emit("\n");
    }
    finish(sink)
}

/// Take a sink's text and spans.
fn finish(sink: Sink) -> Rendered {
    let (text, spans) = sink.finish();
    Rendered { text, spans }
}
