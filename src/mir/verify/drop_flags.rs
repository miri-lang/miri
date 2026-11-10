// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The drop flags of a body, as the verifiers that follow a binding across
//! the edges its flag decides read them.
//!
//! Drop elaboration keeps a `bool` local for each binding it guards
//! ([`FlagOf::Drop`]). A verifier never takes the flag's word for what the
//! binding holds. It follows each flag's value from the constant writes the
//! body makes, keeping one state per combination of values the flags may hold
//! ([`Partitioned`]), and takes the edge a switch on a flag leaves along only
//! with the states whose flag holds the value that leads there. A flag whose
//! value it cannot follow is `Unknown`, and both edges then take the state.
//!
//! What makes the flag's value mean anything is where it is written, and
//! [`flag_write_violations`] holds every write to it: a constant, set right
//! after its binding is bound whole, cleared right after its binding's storage
//! ends or on entry, before anything binds it. Nothing here reads the pass
//! that writes the flags.

use super::{local_display_name, VerificationViolation};
use crate::ast::literal::Literal;
use crate::mir::terminator::Discriminant;
use crate::mir::{
    Body, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use std::collections::BTreeMap;

/// What a path has last written to one drop flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum FlagValue {
    /// Nothing the verifier can follow: not yet written, or written with
    /// something other than a constant.
    Unknown,
    Clear,
    Set,
}

/// The value each drop flag holds along a set of paths, by flag index.
pub(super) type Valuation = Vec<FlagValue>;

/// The drop flags a body declares, each with the binding it guards.
pub(super) struct DropFlags {
    flags: Vec<(Local, Local)>,
}

impl DropFlags {
    pub(super) fn of(body: &Body) -> DropFlags {
        let flags = (body.local_decls.iter().enumerate())
            .filter_map(|(local, decl)| Some((Local(local), decl.drop_flag_of()?)))
            .collect();
        DropFlags { flags }
    }

    /// What every flag holds on entry to the body: nothing written yet.
    pub(super) fn on_entry(&self) -> Valuation {
        vec![FlagValue::Unknown; self.flags.len()]
    }

    /// The index of the flag `local` is, if it is one.
    fn index_of(&self, local: Local) -> Option<usize> {
        self.flags.iter().position(|(flag, _)| *flag == local)
    }

    /// The flag `kind` writes and what it writes: a constant's value, or
    /// `Unknown` for anything else.
    pub(super) fn write_of(&self, kind: &StatementKind) -> Option<(usize, FlagValue)> {
        let place = match kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => place,
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_, _)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::ReadHeader(_)
            | StatementKind::Nop => return None,
        };
        let flag = self.index_of(place.local)?;
        let value = match constant_flag_write(kind) {
            Some(true) => FlagValue::Set,
            Some(false) => FlagValue::Clear,
            None => FlagValue::Unknown,
        };
        Some((flag, value))
    }

    /// Whether the edge `kind` leaves along to `target` can be taken by a
    /// path whose flags hold `valuation`.
    pub(super) fn admits(
        &self,
        valuation: &Valuation,
        kind: &TerminatorKind,
        target: usize,
    ) -> bool {
        let TerminatorKind::SwitchInt {
            discr,
            targets,
            otherwise,
        } = kind
        else {
            return true;
        };
        let Some(flag) = discr.bare_local().and_then(|local| self.index_of(local)) else {
            return true;
        };
        let taken_on = |value: Discriminant| {
            (targets.iter())
                .find(|(listed, _)| *listed == value)
                .map_or(*otherwise, |(_, block)| *block)
        };
        match valuation.get(flag).copied().unwrap_or(FlagValue::Unknown) {
            FlagValue::Unknown => true,
            FlagValue::Clear => taken_on(Discriminant::bool_false()).0 == target,
            FlagValue::Set => taken_on(Discriminant::bool_true()).0 == target,
        }
    }
}

/// The value `kind` writes when it is `flag = true` or `flag = false` written
/// whole with a constant.
fn constant_flag_write(kind: &StatementKind) -> Option<bool> {
    let StatementKind::Assign(place, Rvalue::Use(Operand::Constant(constant))) = kind else {
        return None;
    };
    match constant.literal {
        Literal::Boolean(value) if place.projection.is_empty() => Some(value),
        _ => None,
    }
}

/// A state kept apart for each valuation of the drop flags that reaches a
/// point.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Partitioned<S> {
    parts: BTreeMap<Valuation, S>,
}

impl<S: Clone> Partitioned<S> {
    /// No path reaches the point.
    pub(super) fn unreached() -> Self {
        Partitioned {
            parts: BTreeMap::new(),
        }
    }

    /// The paths reaching the point all hold `valuation` and `state`.
    pub(super) fn single(valuation: Valuation, state: S) -> Self {
        Partitioned {
            parts: BTreeMap::from([(valuation, state)]),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.parts.len()
    }

    pub(super) fn states(&self) -> impl Iterator<Item = &S> {
        self.parts.values()
    }

    pub(super) fn states_mut(&mut self) -> impl Iterator<Item = &mut S> {
        self.parts.values_mut()
    }

    /// Every path now holds `value` in `flag`; paths that come to agree on
    /// every flag are joined by `join`.
    pub(super) fn write(
        &mut self,
        flag: usize,
        value: FlagValue,
        join: impl Fn(&mut S, &S) -> bool,
    ) {
        let parts = std::mem::take(&mut self.parts);
        for (mut valuation, state) in parts {
            if let Some(slot) = valuation.get_mut(flag) {
                *slot = value;
            }
            self.merge(valuation, &state, &join);
        }
    }

    /// The paths that can take the edge `kind` leaves along to `target`.
    pub(super) fn along_edge(
        &self,
        flags: &DropFlags,
        kind: &TerminatorKind,
        target: usize,
    ) -> Self {
        let parts = (self.parts.iter())
            .filter(|(valuation, _)| flags.admits(valuation, kind, target))
            .map(|(valuation, state)| (valuation.clone(), state.clone()))
            .collect();
        Partitioned { parts }
    }

    /// Join `other` into these paths, valuation by valuation; whether any
    /// state changed.
    pub(super) fn join(&mut self, other: &Self, join: impl Fn(&mut S, &S) -> bool) -> bool {
        let mut changed = false;
        for (valuation, state) in &other.parts {
            changed |= self.merge(valuation.clone(), state, &join);
        }
        changed
    }

    fn merge(
        &mut self,
        valuation: Valuation,
        state: &S,
        join: &impl Fn(&mut S, &S) -> bool,
    ) -> bool {
        match self.parts.get_mut(&valuation) {
            Some(existing) => join(existing, state),
            None => {
                self.parts.insert(valuation, state.clone());
                true
            }
        }
    }
}

/// Every write to a drop flag that does not keep the flag's meaning: a write
/// of anything but a constant, a set anywhere but right after its binding is
/// bound whole, and a clear anywhere but right after its binding's storage
/// ends or on entry, before anything binds it.
pub(super) fn flag_write_violations(body: &Body) -> Vec<VerificationViolation> {
    let mut violations = Vec::new();
    let predecessors = crate::mir::analysis::loops::predecessors(body);
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for (index, statement) in data.statements.iter().enumerate() {
            let Some(place) = statement.written_place() else {
                continue;
            };
            let Some(binding) = body.local_decls[place.local.0].drop_flag_of() else {
                continue;
            };
            let preceding = &data.statements[..index];
            let refusal = match constant_flag_write(&statement.kind) {
                None => Some("is written only with a constant `true` or `false`"),
                Some(true)
                    if !is_set_after_binding(body, &predecessors, block, preceding, binding) =>
                {
                    Some("is set only right after the binding it guards is bound whole")
                }
                Some(false) if !is_cleared_after_end(block, preceding, binding) => Some(
                    "is cleared only right after the storage of the binding it guards ends, \
                     or on entry",
                ),
                Some(_) => None,
            };
            if let Some(refusal) = refusal {
                violations.push(flag_violation(body, place.local, block, index, refusal));
            }
        }
        let written = data.terminator.iter().flat_map(Terminator::written_places);
        for place in
            written.filter(|place| body.local_decls[place.local.0].drop_flag_of().is_some())
        {
            let refusal = "is written only with a constant `true` or `false`";
            violations.push(flag_violation(
                body,
                place.local,
                block,
                data.statements.len(),
                refusal,
            ));
        }
    }
    violations
}

fn flag_violation(
    body: &Body,
    flag: Local,
    block: usize,
    index: usize,
    refusal: &str,
) -> VerificationViolation {
    VerificationViolation {
        local: flag,
        local_name: local_display_name(body, flag),
        message: format!("a drop flag written at bb{block}[{index}] {refusal}"),
    }
}

/// The statement before a flag write that decides what it may write: the
/// nearest one that is not a retain or a release, which reference counting
/// places around a binding's own statements.
fn deciding_statement(preceding: &[Statement]) -> Option<&StatementKind> {
    (preceding.iter().rev())
        .map(|statement| &statement.kind)
        .find(|kind| {
            !matches!(
                kind,
                StatementKind::IncRef(_)
                    | StatementKind::DecRef(_)
                    | StatementKind::Dealloc(_)
                    | StatementKind::Nop
            )
        })
}

/// Whether a set of `binding`'s flag at the end of `preceding`, in `block`,
/// follows a binding of `binding` whole: a statement assigning it, or, first
/// in its block, a terminator of every predecessor writing it.
fn is_set_after_binding(
    body: &Body,
    predecessors: &[Vec<usize>],
    block: usize,
    preceding: &[Statement],
    binding: Local,
) -> bool {
    let whole = Place::new(binding);
    if preceding.is_empty() {
        let incoming = predecessors
            .get(block)
            .map(Vec::as_slice)
            .unwrap_or_default();
        return !incoming.is_empty()
            && incoming.iter().all(|from| {
                (body.basic_blocks[*from].terminator.iter())
                    .flat_map(Terminator::written_places)
                    .any(|place| *place == whole)
            });
    }
    matches!(
        deciding_statement(preceding),
        Some(StatementKind::Assign(place, _) | StatementKind::Reassign(place, _)) if *place == whole
    )
}

/// Whether a clear of `binding`'s flag at the end of `preceding`, in `block`,
/// follows the end of `binding`'s storage, or sits in the entry block before
/// anything binds or ends `binding`.
fn is_cleared_after_end(block: usize, preceding: &[Statement], binding: Local) -> bool {
    let whole = Place::new(binding);
    if matches!(deciding_statement(preceding), Some(StatementKind::StorageDead(place, _)) if *place == whole)
    {
        return true;
    }
    block == 0
        && !preceding.iter().any(|statement| match &statement.kind {
            StatementKind::Assign(place, _)
            | StatementKind::Reassign(place, _)
            | StatementKind::StorageLive(place)
            | StatementKind::StorageDead(place, _) => place.local == binding,
            StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::ReadHeader(_)
            | StatementKind::Nop => false,
        })
}
