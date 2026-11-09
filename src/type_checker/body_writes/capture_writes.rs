// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which writes in a closure or nested function would be lost.
//!
//! A closure holds a copy of each value it captures, made when the closure is
//! created, and every call starts over from it. A write to that copy is
//! therefore gone when the call ends, so it is refused where it is written
//! (`MER_TYP_090`).
//!
//! The type checker knows the scopes, so it records which mentions are of a
//! captured binding as it resolves them ([`CapturedMentions`]). This pass
//! then reads the one walk of mentions ([`super::BodyWrites`]) and the one
//! rule of what writes ([`super::parameters::is_written`]) and refuses a
//! mention that is both. A write through a class instance or a trait-typed
//! value somewhere along the written path is allowed: the capture is a
//! reference, and the write persists.

use std::collections::HashMap;
use std::rc::Rc;

use super::parameters::is_written;
use super::{argument_value, BodyWrites, Mention, Role};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::syntax::Span;
use crate::type_checker::context::{CapturedBinding, Context};
use crate::type_checker::TypeChecker;

/// The file an imported module's diagnostics point at: its path and text.
type SourceOverride = (String, String);

/// One mention of a binding a closure or nested function captures.
#[derive(Debug, Clone)]
struct CapturedMention {
    binding: CapturedBinding,
    source: Option<Rc<SourceOverride>>,
}

/// Every mention of a captured binding, by the id of the mentioning
/// expression.
#[derive(Debug, Default)]
pub struct CapturedMentions {
    mentions: HashMap<usize, CapturedMention>,
    /// The source the last mention was recorded under, shared by the
    /// mentions after it in the same module.
    current_source: Option<Rc<SourceOverride>>,
}

impl CapturedMentions {
    /// Records the expression `id` as a mention of the captured `binding`,
    /// checked while the source override `source` was in effect.
    pub(crate) fn record(
        &mut self,
        id: usize,
        binding: CapturedBinding,
        source: Option<&SourceOverride>,
    ) {
        if self.current_source.as_deref() != source {
            self.current_source = source.cloned().map(Rc::new);
        }
        let source = self.current_source.clone();
        self.mentions
            .insert(id, CapturedMention { binding, source });
    }
}

/// A write to a captured value that would be lost when the call ends.
#[derive(Debug)]
pub struct LostCaptureWrite {
    name: String,
    span: Span,
    source: Option<Rc<SourceOverride>>,
}

/// The writes of every body of a program that would be lost.
#[derive(Debug, Default)]
pub struct LostCaptureWrites {
    writes: Vec<LostCaptureWrite>,
}

impl LostCaptureWrites {
    /// Records the writes of captured values among the mentions of `writes`.
    pub(super) fn record(&mut self, writes: &BodyWrites, checker: &TypeChecker, context: &Context) {
        for mention in &writes.mentions {
            let Some(captured) = checker
                .captured_mentions
                .mentions
                .get(&mention.expression.id)
            else {
                continue;
            };
            if is_lost_write(mention, captured.binding, checker, context) {
                self.writes.push(LostCaptureWrite {
                    name: mention.name().to_string(),
                    span: mention.expression.span,
                    source: captured.source.clone(),
                });
            }
        }
    }
}

/// Whether `mention` of a captured binding writes the closure's own copy.
///
/// An assignment to a `let` binding already reports that it cannot be
/// assigned at all, so it is not reported a second time here.
fn is_lost_write(
    mention: &Mention,
    binding: CapturedBinding,
    checker: &TypeChecker,
    context: &Context,
) -> bool {
    let Some(role) = mention.role else {
        return false;
    };
    let is_refused_assignment = matches!(role, Role::Assigned { .. }) && !binding.mutable;
    !is_refused_assignment
        && is_written(checker, mention)
        && !writes_through_reference(role, checker, context)
}

/// Whether a step of the path `role` writes holds a reference, so the write
/// reaches the object that reference names rather than the closure's copy.
fn writes_through_reference(role: Role<'_>, checker: &TypeChecker, context: &Context) -> bool {
    let mut place = written_place(role);
    while let Some(inner) = place.and_then(step_base) {
        if checker.holds_by_reference(inner, context) {
            return true;
        }
        place = Some(inner);
    }
    false
}

/// The place `role` writes.
fn written_place(role: Role<'_>) -> Option<&Expression> {
    match role {
        Role::Assigned { target } => Some(target),
        Role::Receiver { object, .. } => Some(object),
        Role::Argument {
            arguments, index, ..
        } => arguments.get(index).map(|arg| argument_value(arg).0),
    }
}

/// The value the path step `place` steps into, when it is a field or element.
fn step_base(place: &Expression) -> Option<&Expression> {
    if let ExpressionKind::Member(base, _) | ExpressionKind::Index(base, _) = &place.node {
        return Some(base);
    }
    None
}

impl TypeChecker {
    /// Reports `MER_TYP_090` for each write of a captured value, each in the
    /// source of the module it was checked in.
    pub(crate) fn report_lost_capture_writes(&mut self, lost: LostCaptureWrites) {
        for write in lost.writes {
            let previous = std::mem::replace(
                &mut self.modules.current_source_override,
                write.source.as_deref().cloned(),
            );
            self.report_error_with_help(
                crate::diagnostics::DiagnosticCode::TypCapturedValueWritten,
                format!(
                    "a write to '{}' would be lost when the call ends: the closure holds its own copy, made when it was created",
                    write.name
                ),
                write.span,
                "hold the state in a class: a closure that captures an instance shares it, so a write through it persists".to_string(),
            );
            self.modules.current_source_override = previous;
        }
    }
}
