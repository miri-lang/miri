// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders a parsed program back to canonical Miri source.
//!
//! The rendering is canonical: it is derived from the AST, so blank lines and
//! the author's spacing are normalized away and one program shape always
//! produces one text. That is what lets a tool read a declaration here and
//! anchor an edit against the same bytes later.
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
pub fn program(program: &Program) -> Rendered {
    let mut sink = Sink::with_comments();
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
pub fn declaration(node: &Statement) -> Rendered {
    let mut sink = Sink::new();
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
