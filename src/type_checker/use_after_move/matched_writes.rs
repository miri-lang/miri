// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A write to the value a `match` reads ends the bindings its arm's patterns
//! read out of it.
//!
//! A payload binding reads the matched value's payload in place: it is a
//! borrow, kept alive by the matched place. Assigning that place, or a place
//! containing it, handing either to an `out` parameter, or calling on either
//! a method that writes its receiver replaces the value and releases the
//! payload the binding reads. A later use would read a released resource, or
//! need the binding to hold a reference of its own, a second holder whose
//! hook runs out of program order. So the write ends every payload binding
//! read out of the place it writes, and a later use of one whose type reaches
//! a drop hook is refused, with the repair: use the binding before the
//! write, or write after the match. A binding of a plain value shares the
//! old payload and is left alone.
//!
//! Places are compared as the containment check of a call compares them: two
//! paths from one root meet when every step they share names the same field
//! or indexes a collection, since two indices may be equal. A write beside
//! the matched place, to a sibling field, leaves its bindings standing.
//!
//! The consumption state carries the ending, so it flows through branches
//! and around loops the way a consumption does: a binding read again on the
//! next pass of a loop that writes the matched value is refused as well.

use std::collections::HashSet;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::place_path::{path_links, PathLink};
use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::place_reads::value_spelling;

use super::{argument_value, Consumed, ConsumedInfo, UseAfterMoveChecker};

// TODO: a call lending the matched value to a function that writes it
// through a mutating method (`reset(p)` where `reset` calls `x.clear()`) is
// not a write here, because such a call does not yet write through to the
// caller's place; once it does, it ends the payload bindings the same way.

/// One step of a place path: a named field, or an element of a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Field(String),
    Element,
}

/// A place a program spells: a binding and the steps taken from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlaceKey {
    root: String,
    steps: Vec<Step>,
}

impl PlaceKey {
    /// The place `expression` spells, when it is a binding, `self`, or a
    /// path of fields and elements from one.
    pub(super) fn of(expression: &Expression) -> Option<Self> {
        let (root, links) = path_links(expression);
        let ExpressionKind::Identifier(root, _) = &root.node else {
            return None;
        };
        let steps = (links.iter())
            .map(|link| match link {
                PathLink::Member(_, member, _) => {
                    let ExpressionKind::Identifier(field, _) = &member.node else {
                        return None;
                    };
                    Some(Step::Field(field.clone()))
                }
                PathLink::Index(..) => Some(Step::Element),
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            root: root.clone(),
            steps,
        })
    }

    /// Whether a write to `self` may reach `other`: one root, and every
    /// step both take may be the same, so one path holds the other.
    fn meets(&self, other: &Self) -> bool {
        self.root == other.root
            && (self.steps.iter())
                .zip(&other.steps)
                .all(|(mine, theirs)| mine == theirs)
    }
}

/// A `match` arm whose body is being checked: the place its subject reads,
/// spelled and as a key, and the names its patterns bind.
pub(super) struct MatchedArm {
    pub(super) place: PlaceKey,
    pub(super) spelled: String,
    pub(super) bindings: HashSet<String>,
}

/// A write that ends payload bindings: what it writes, and how a refusal
/// names it.
struct Write<'e> {
    place: PlaceKey,
    spelled: String,
    span: crate::error::syntax::Span,
    /// The arguments of the call making the write, which the call reads
    /// while it writes.
    arguments: &'e [Expression],
}

impl UseAfterMoveChecker<'_> {
    /// Enters the arm binding `bindings` out of the place `subject` reads,
    /// when the subject is a place; returns whether an arm was entered.
    pub(super) fn enter_matched_arm(
        &mut self,
        subject: &Expression,
        bindings: &HashSet<String>,
    ) -> bool {
        let Some(place) = PlaceKey::of(subject) else {
            return false;
        };
        self.matched_arms.push(MatchedArm {
            place,
            spelled: value_spelling(subject),
            bindings: bindings.clone(),
        });
        true
    }

    /// `name` is bound afresh: it no longer names a payload binding, and a
    /// write through it no longer reaches a matched value rooted at the
    /// binding it shadows.
    pub(super) fn forget_matched_binding(&mut self, name: &str) {
        for arm in &mut self.matched_arms {
            arm.bindings.remove(name);
            if arm.place.root == name {
                arm.bindings.clear();
            }
        }
    }

    /// Ends the payload bindings the assignment to `target` writes.
    pub(super) fn end_bindings_assigned(&mut self, target: &Expression, consumed: &mut Consumed) {
        let Some(place) = self.written_place(target) else {
            return;
        };
        let write = Write {
            place,
            spelled: format!("the assignment to `{}`", value_spelling(target)),
            span: target.span,
            arguments: &[],
        };
        self.end_written_bindings(&write, consumed);
    }

    /// Ends the payload bindings the call `call` of `callee` with `args`
    /// writes: through its receiver, when the method writes it, and through
    /// each argument bound to an `out` parameter.
    pub(super) fn end_bindings_written_by_call(
        &mut self,
        call: &Expression,
        (callee, args): (&Expression, &[Expression]),
        consumed: &mut Consumed,
    ) {
        if self.matched_arms.is_empty() {
            return;
        }
        let mut writes = Vec::new();
        if let Some((receiver, method)) = self.written_receiver(callee) {
            if let Some(place) = PlaceKey::of(receiver) {
                let spelled = format!("`{}.{method}()`", value_spelling(receiver));
                writes.push((place, spelled));
            }
        }
        for value in args.iter().map(argument_value) {
            let binds_out = (self.out_arguments)
                .and_then(|outs| outs.recorded_binding(call.id, value.id))
                .unwrap_or(false);
            if let Some(place) = PlaceKey::of(value).filter(|_| binds_out) {
                let spelled = format!("`{}({})`", callee_spelling(callee), value_spelling(value));
                writes.push((place, spelled));
            }
        }
        for (place, spelled) in writes {
            let write = Write {
                place,
                spelled,
                span: call.span,
                arguments: args,
            };
            self.end_written_bindings(&write, consumed);
        }
    }

    /// The receiver and method name of the method call `callee`, when the
    /// method may write its receiver. An enum changes only by replacing
    /// its whole value, so an enum's method, an optional's included, writes
    /// exactly when it replaces its receiver; a class's method writes when
    /// its body may, or was never analysed.
    fn written_receiver<'e>(&self, callee: &'e Expression) -> Option<(&'e Expression, &'e str)> {
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return None;
        };
        let ExpressionKind::Identifier(method, _) = &method.node else {
            return None;
        };
        let restored = self.vtable_calls.restored;
        let kind = &self.types.get(&receiver.id)?.kind;
        let writes = if let TypeKind::Option(_) = kind {
            let optional = self.repairs?.optional.as_ref()?;
            restored.replaces_receiver(&optional.name, method)
        } else if let TypeKind::Custom(name, _) = kind {
            match self.type_definitions.get(name) {
                Some(TypeDefinition::Enum(_)) => restored.replaces_receiver(name, method),
                Some(
                    TypeDefinition::Struct(_)
                    | TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_)
                    | TypeDefinition::Class(_)
                    | TypeDefinition::Trait(_),
                )
                | None => restored.writes_receiver(name, method),
            }
        } else {
            false
        };
        writes.then_some((receiver, method.as_str()))
    }

    /// The place an assignment to `target` writes, when a payload binding
    /// is live to be ended by it.
    fn written_place(&self, target: &Expression) -> Option<PlaceKey> {
        if self.matched_arms.is_empty() {
            return None;
        }
        PlaceKey::of(target)
    }

    /// Ends every payload binding `write` reaches: those of an arm whose
    /// subject place it meets, and of an arm whose subject is itself a
    /// binding ended here, each only while it is the binding its name
    /// stands for. Refuses a resource binding the writing call reads.
    fn end_written_bindings(&mut self, write: &Write<'_>, consumed: &mut Consumed) {
        let ended = self.bindings_ended_by(&write.place);
        for (name, scrutinee) in &ended {
            consumed
                .entry(name.clone())
                .or_insert_with(|| ConsumedInfo {
                    by_fn: write.spelled.clone(),
                    at_span: write.span,
                    chain: Vec::new(),
                    open: None,
                    ended_by_write: Some(scrutinee.clone()),
                });
        }
        for value in write.arguments.iter().map(argument_value) {
            let read =
                (PlaceKey::of(value)).and_then(|key| ended.iter().find(|(n, _)| *n == key.root));
            if let Some((name, scrutinee)) = read.filter(|_| self.is_resource_expr(value)) {
                let error = lent_binding_error((name, scrutinee), &write.spelled, value.span);
                self.errors.push(error);
            }
        }
    }

    /// The names a write to `place` ends, outermost arm first, each with
    /// the matched value the write reaches it through: its own arm's, or,
    /// for an arm matching a binding ended here, the one that binding was
    /// read out of.
    fn bindings_ended_by(&self, place: &PlaceKey) -> Vec<(String, String)> {
        let mut ended: Vec<(String, String)> = Vec::new();
        for (depth, arm) in self.matched_arms.iter().enumerate() {
            let through = (ended.iter())
                .find(|(name, _)| *name == arm.place.root)
                .map(|(_, matched)| matched.clone());
            let matched = match through {
                _ if place.meets(&arm.place) => arm.spelled.clone(),
                Some(matched) => matched,
                None => continue,
            };
            let shadowed = |name: &String| {
                (self.matched_arms[depth + 1..].iter()).any(|inner| inner.bindings.contains(name))
            };
            for name in arm.bindings.iter().filter(|name| !shadowed(name)) {
                ended.push((name.clone(), matched.clone()));
            }
        }
        ended
    }

    /// Refuses the use `expression` of `name`, a payload binding a write
    /// recorded in `info` ended, when its type reaches a drop hook.
    pub(super) fn refuse_use_after_matched_write(
        &mut self,
        (name, expression): (&str, &Expression),
        info: &ConsumedInfo,
    ) {
        let Some(scrutinee) = &info.ended_by_write else {
            return;
        };
        if !self.is_resource_expr(expression) {
            return;
        }
        let note = RelatedNote::at(
            format!("{} writes `{scrutinee}` here", info.by_fn),
            info.at_span,
        );
        let error =
            ended_binding_error((name, scrutinee), &info.by_fn, expression.span, Some(note));
        self.errors.push(error);
    }
}

/// How a refusal names the function or method `callee` calls.
fn callee_spelling(callee: &Expression) -> String {
    if let ExpressionKind::Identifier(name, _) = &callee.node {
        return name.clone();
    }
    if let ExpressionKind::Member(receiver, method) = &callee.node {
        return format!("{}.{}", value_spelling(receiver), value_spelling(method));
    }
    "the call".to_string()
}

/// The refusal of a use, at `span`, of the payload binding `name` read out
/// of `scrutinee` after `write` wrote it.
fn ended_binding_error(
    (name, scrutinee): (&str, &str),
    write: &str,
    span: crate::error::syntax::Span,
    note: Option<RelatedNote>,
) -> TypeError {
    let message = format!(
        "`{name}` is used after {write} writes `{scrutinee}`, the value its arm matched: the write releases the resource `{name}` reads"
    );
    let help = format!("use `{name}` before {write}, or write `{scrutinee}` after the match");
    refusal(message, help, span, note)
}

/// The refusal of handing the payload binding `name`, read out of
/// `scrutinee`, at `span`, to `call`, which writes `scrutinee`.
fn lent_binding_error(
    (name, scrutinee): (&str, &str),
    call: &str,
    span: crate::error::syntax::Span,
) -> TypeError {
    let message = format!(
        "`{name}` is handed to {call}, which writes `{scrutinee}`, the value its arm matched: the write releases the resource `{name}` reads while the call holds it"
    );
    let help = format!(
        "read what the call needs from `{name}` before it, or write `{scrutinee}` after the match"
    );
    refusal(message, help, span, None)
}

/// A refusal of a payload binding a write to its matched value ended.
fn refusal(
    message: String,
    help: String,
    span: crate::error::syntax::Span,
    note: Option<RelatedNote>,
) -> TypeError {
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnPayloadUsedAfterMatchedWrite,
            message,
            help: Some(help),
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: note.into_iter().collect(),
    }
}
