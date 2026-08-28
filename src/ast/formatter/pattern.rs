// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders match patterns back to Miri source syntax.

use crate::ast::pattern::Pattern;

use super::helpers::literal;
use super::sink::Sink;

/// Render one pattern.
pub fn pattern(sink: &mut Sink, node: &Pattern) {
    match node {
        Pattern::Literal(value) => literal(sink, value),
        Pattern::Identifier(name) => sink.emit(name),
        Pattern::Default => sink.emit("default"),
        Pattern::Tuple(members) => {
            sink.emit("(");
            comma_separated(sink, members);
            sink.emit(")");
        }
        Pattern::Regex(regex) => literal(sink, &crate::ast::literal::Literal::Regex(regex.clone())),
        Pattern::Member(base, member) => {
            pattern(sink, base);
            sink.emit(".");
            sink.emit(member);
        }
        Pattern::EnumVariant(path, bindings) => {
            pattern(sink, path);
            if bindings.is_empty() {
                return;
            }
            sink.emit("(");
            comma_separated(sink, bindings);
            sink.emit(")");
        }
    }
}

/// Render patterns separated by `, `.
fn comma_separated(sink: &mut Sink, patterns: &[Pattern]) {
    for (index, entry) in patterns.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        pattern(sink, entry);
    }
}
