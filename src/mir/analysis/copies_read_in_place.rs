// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The copies of a value held as its bytes that every read can take from
//! where the value was copied from.
//!
//! `t = p`, with `p` a projected place — an element, a field, a payload —
//! copies a value's bytes out of memory into the local `t`. When nothing can
//! change those bytes, or which bytes `p` names, between the copy and any
//! read of `t`, each read of `t` sees what the same read of `p` would. Code
//! generation then lets `t` name the bytes at `p` and loads each field where
//! it is read, instead of loading every field at the copy and keeping them
//! all alive until the last read: a match on `code[pc]` reads the element's
//! address once and each arm loads only the payload it binds.
//!
//! A local qualifies when it holds a value the body keeps as its bytes, is
//! neither a parameter nor the result nor a capture, and its one write is
//! that copy: nothing else assigns it or any part of it, takes its address,
//! or names it in a terminator, whose callee could keep the address past a
//! write. A forward must-dataflow carries the qualified locals whose source
//! still holds what was copied: a local joins at its copy and leaves when
//!
//! - a statement writes through a projection, a release or deallocation
//!   runs, or a call or a kernel launch is made — any of them may write the
//!   source's bytes, free them, or run code of the program that does;
//! - a local the source place is rooted at or indexed by is given a value or
//!   has its storage started or ended, so the place names other bytes;
//! - the storage of a local with a device buffer ends, which releases it.
//!
//! A local read anywhere it has left the set is a real copy.

use std::collections::{HashMap, HashSet};

use crate::mir::analysis::traversal::reverse_postorder;
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Place, Rvalue, Statement, StatementKind, TerminatorKind,
};

/// Every local of `body` assigned once, by a copy out of a projected place,
/// whose every read happens while the place still holds what was copied.
pub fn copies_read_in_place(body: &Body) -> HashSet<Local> {
    let sources = candidate_sources(body);
    if sources.is_empty() {
        return HashSet::new();
    }
    let flow = Flow { body, sources };
    let entries = flow.solve();
    let mut read_in_place: HashSet<Local> = flow.sources.keys().copied().collect();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let Some(Some(mut valid)) = entries.get(block).cloned() else {
            continue;
        };
        for statement in &data.statements {
            for read in flow.candidate_reads(statement) {
                if !valid.contains(&read) {
                    read_in_place.remove(&read);
                }
            }
            flow.apply(statement, &mut valid);
        }
    }
    read_in_place
}

/// Each local that may be read in place, with the place it copies.
fn candidate_sources(body: &Body) -> HashMap<Local, Place> {
    let mut writes: HashMap<Local, Vec<&Statement>> = HashMap::new();
    let mut excluded: HashSet<Local> = (0..=body.arg_count).map(Local).collect();
    excluded.extend(body.env_capture_locals.iter().copied());
    for data in &body.basic_blocks {
        for statement in &data.statements {
            if let StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) =
                &statement.kind
            {
                writes.entry(place.local).or_default().push(statement);
                if let Rvalue::Ref(taken) = rvalue {
                    excluded.insert(taken.local);
                }
            }
        }
        if let Some(terminator) = &data.terminator {
            let mut named = MentionedLocals::default();
            named.visit_terminator(BasicBlock(0), terminator);
            excluded.extend(named.locals);
        }
    }
    writes
        .into_iter()
        .filter(|(local, _)| !excluded.contains(local) && is_held(body, *local))
        .filter_map(|(local, writes)| {
            let [only] = writes.as_slice() else {
                return None;
            };
            copied_source(only, local).map(|source| (local, source))
        })
        .collect()
}

/// The place `statement` copies whole into `local`, when it is that copy out
/// of a projected place not involving `local` itself.
fn copied_source(statement: &Statement, local: Local) -> Option<Place> {
    let StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(source) | Operand::Move(source))) =
        &statement.kind
    else {
        return None;
    };
    let is_copy =
        dest.projection.is_empty() && !source.projection.is_empty() && !mentions(source, local);
    is_copy.then(|| source.clone())
}

fn is_held(body: &Body, local: Local) -> bool {
    body.local_decls
        .get(local.0)
        .is_some_and(|decl| body.value_layout(&decl.ty.kind).is_some())
}

/// Whether `place` is rooted at or indexed by `local`.
fn mentions(place: &Place, local: Local) -> bool {
    place.local == local || place.index_locals().any(|index| index == local)
}

/// Every local a statement or terminator names, as a root or an index.
#[derive(Default)]
struct MentionedLocals {
    locals: Vec<Local>,
}

impl Visitor for MentionedLocals {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.locals.push(local);
    }
}

type Valid = HashSet<Local>;

/// The body the dataflow runs over, with the candidates and their sources.
struct Flow<'a> {
    body: &'a Body,
    sources: HashMap<Local, Place>,
}

impl Flow<'_> {
    /// The locals valid on entry to each block; `None` for a block no path
    /// reaches.
    fn solve(&self) -> Vec<Option<Valid>> {
        let count = self.body.basic_blocks.len();
        let mut entries: Vec<Option<Valid>> = vec![None; count];
        let Some(first) = entries.first_mut() else {
            return entries;
        };
        *first = Some(Valid::new());
        let mut pending: Vec<usize> = reverse_postorder(self.body);
        pending.reverse();
        while let Some(block) = pending.pop() {
            let (Some(Some(mut valid)), Some(data)) = (
                entries.get(block).cloned(),
                self.body.basic_blocks.get(block),
            ) else {
                continue;
            };
            for statement in &data.statements {
                self.apply(statement, &mut valid);
            }
            let Some(terminator) = &data.terminator else {
                continue;
            };
            if writes_memory(&terminator.kind) {
                valid.clear();
            }
            for successor in terminator.successors() {
                let Some(slot) = entries.get_mut(successor.0) else {
                    continue;
                };
                let merged = match slot {
                    None => valid.clone(),
                    Some(old) => old.intersection(&valid).copied().collect(),
                };
                if slot.as_ref() != Some(&merged) {
                    *slot = Some(merged);
                    pending.push(successor.0);
                }
            }
        }
        entries
    }

    /// The candidates `statement` reads; the copy that defines one is not a
    /// read of it.
    fn candidate_reads(&self, statement: &Statement) -> Vec<Local> {
        let mut named = MentionedLocals::default();
        let defined = match &statement.kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                named.visit_assign(BasicBlock(0), dest, rvalue);
                dest.projection.is_empty().then_some(dest.local)
            }
            StatementKind::IncRef(_) | StatementKind::DecRef(_) | StatementKind::Dealloc(_) => {
                named.visit_statement(BasicBlock(0), statement);
                None
            }
            // Starting or ending a local's storage reads nothing of it.
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                None
            }
        };
        named
            .locals
            .into_iter()
            .filter(|local| Some(*local) != defined && self.sources.contains_key(local))
            .collect()
    }

    /// `valid` after `statement`.
    fn apply(&self, statement: &Statement, valid: &mut Valid) {
        match &statement.kind {
            StatementKind::Assign(dest, _) | StatementKind::Reassign(dest, _) => {
                if !dest.projection.is_empty() {
                    valid.clear();
                    return;
                }
                self.forget_sources_naming(dest.local, valid);
                if self.sources.contains_key(&dest.local) {
                    valid.insert(dest.local);
                }
            }
            StatementKind::StorageLive(place) => self.forget_sources_naming(place.local, valid),
            StatementKind::StorageDead(place) => {
                let has_device_buffer = (self.body.local_decls.get(place.local.0))
                    .is_some_and(|decl| decl.device_handle.is_some());
                if has_device_buffer {
                    valid.clear();
                } else {
                    self.forget_sources_naming(place.local, valid);
                }
            }
            StatementKind::DecRef(_) | StatementKind::Dealloc(_) => valid.clear(),
            StatementKind::IncRef(_) | StatementKind::Nop => {}
        }
    }

    /// Drop from `valid` every candidate whose source is rooted at or
    /// indexed by `local`, and `local` itself.
    fn forget_sources_naming(&self, local: Local, valid: &mut Valid) {
        valid.retain(|candidate| {
            *candidate != local
                && self
                    .sources
                    .get(candidate)
                    .is_some_and(|source| !mentions(source, local))
        });
    }
}

/// Whether a terminator of `kind` may write memory or run code that does.
fn writes_memory(kind: &TerminatorKind) -> bool {
    match kind {
        TerminatorKind::Call { .. }
        | TerminatorKind::VirtualCall { .. }
        | TerminatorKind::GpuLaunch { .. } => true,
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => false,
    }
}
