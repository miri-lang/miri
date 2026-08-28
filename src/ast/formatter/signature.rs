// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders a declaration's header without its body.
//!
//! An outline is the headers alone, so the same header rendering the full
//! formatter uses is reused here and the body is simply never asked for. That
//! keeps one spelling of a signature rather than two that can drift.

use crate::ast::statement::{Statement, StatementKind};

use super::sink::Sink;
use super::statement as full;

/// One declaration reduced to its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// What kind of declaration this is, such as `function` or `class`.
    pub kind: &'static str,
    /// The declared name.
    pub name: String,
    /// The header as canonical source, with no body.
    pub text: String,
}

/// The header of `node`, or `None` when `node` declares nothing.
pub fn signature(node: &Statement) -> Option<Signature> {
    match &node.node {
        StatementKind::FunctionDeclaration(declaration) => {
            let mut sink = Sink::new();
            full::function_header(&mut sink, declaration, 0);
            Some(built("function", declaration.name.clone(), sink))
        }
        StatementKind::Class(data) => {
            let mut sink = Sink::new();
            full::class_header(&mut sink, data, 0);
            Some(built("class", rendered_name(&data.name), sink))
        }
        StatementKind::Enum(name, generics, _, _, level, attributes) => {
            let mut sink = Sink::new();
            full::enum_header(&mut sink, name, generics.as_deref(), level, attributes, 0);
            Some(built("enum", rendered_name(name), sink))
        }
        StatementKind::Struct(name, generics, _, _, level, traits) => {
            let mut sink = Sink::new();
            full::struct_header(&mut sink, name, generics.as_deref(), level, traits, 0);
            Some(built("struct", rendered_name(name), sink))
        }
        StatementKind::Trait(name, generics, parents, _, level) => {
            let mut sink = Sink::new();
            full::trait_header(&mut sink, name, generics.as_deref(), parents, level, 0);
            Some(built("trait", rendered_name(name), sink))
        }
        // These declare no body, so the whole statement already is the header.
        StatementKind::RuntimeFunctionDeclaration(_, name, _, _) => {
            Some(whole("function", name.clone(), node))
        }
        StatementKind::IntrinsicFunctionDeclaration(name, _, _, _, _) => {
            Some(whole("function", name.clone(), node))
        }
        StatementKind::Type(declarations, _) => {
            let name = declarations.first().map(rendered_name).unwrap_or_default();
            Some(whole("type", name, node))
        }
        StatementKind::Use(path, _) => Some(whole("use", rendered_name(path), node)),
        // Not declarations: an outline has nothing to say about them.
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Block(_)
        // A field is a member but not a declaration: the grammar admits it
        // through `field_declaration`, beside `declaration`, not within it.
        | StatementKind::Variable(..)
        | StatementKind::If(..)
        | StatementKind::While(..)
        | StatementKind::For(..)
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::Return(_) => None,
    }
}

/// Build a signature from a sink holding only the header.
fn built(kind: &'static str, name: String, sink: Sink) -> Signature {
    let (text, _) = sink.finish();
    Signature { kind, name, text }
}

/// Build a signature from a statement that is entirely a header.
fn whole(kind: &'static str, name: String, node: &Statement) -> Signature {
    let mut sink = Sink::new();
    full::statement(&mut sink, node, 0);
    built(kind, name, sink)
}

/// Render a name expression to text.
fn rendered_name(name: &crate::ast::expression::Expression) -> String {
    let mut sink = Sink::new();
    super::expression::expression(&mut sink, name, 0);
    sink.finish().0
}
