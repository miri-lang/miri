// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every access a body records as in bounds is proven so.
//!
//! Code generation emits a recorded access without comparing its index to
//! the collection's length, so a record nothing proves reads or writes past
//! the end. Each entry is re-derived here from the body alone, with a
//! dataflow of its own: a length is known of a collection from `Len` until
//! the length local or the collection is given another value (a
//! copy-on-write or clone of the collection, or a whole-local copy, carries
//! it), its storage ends, or a call outside the header-preserving runtime
//! entry points runs; an index is known in bounds on the taken edge of a
//! switch on `index < length` whose operands keep their values to the switch,
//! when the index is never negative, and until the index or the collection's
//! length is lost. The access must also sit where the entry says and lie in
//! a block a stable-collection record of the collection covers, since it is
//! that record's length code generation compares against no more.

use super::VerificationViolation;
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::mir::analysis::header_reach::runtime_call_leaves_headers;
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BinOp, Body, Discriminant, InBounds, Local, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::preserves_length;
use std::collections::{HashMap, HashSet};

/// `(length or index local, collection local, is_index)`.
type Known = (usize, usize, bool);

/// Every entry of [`Body::in_bounds`] the body does not prove.
pub fn verify_in_bounds(body: &Body) -> Vec<VerificationViolation> {
    if body.in_bounds.is_empty() {
        return Vec::new();
    }
    let check = Check::of(body);
    let entries = check.entry_states();
    body.in_bounds
        .iter()
        .filter_map(|proof| {
            check
                .failure(proof, &entries)
                .map(|reason| VerificationViolation {
                    local: proof.collection,
                    local_name: proof.collection.to_string(),
                    message: format!(
                        "`{}[{}]` at {}[{}] is recorded in bounds, but {reason}",
                        proof.collection, proof.index, proof.block, proof.statement_index
                    ),
                })
        })
        .collect()
}

struct Check<'a> {
    body: &'a Body,
    never_negative: HashSet<usize>,
    predecessors: Vec<Vec<(usize, Option<Known>)>>,
}

impl<'a> Check<'a> {
    fn of(body: &'a Body) -> Self {
        let never_negative = never_negative(body);
        let mut check = Self {
            body,
            never_negative,
            predecessors: vec![Vec::new(); body.basic_blocks.len()],
        };
        check.predecessors = check.edges();
        check
    }

    /// For each block, every edge into it with the index fact the edge's
    /// guard may add, by the collection and index locals it names later.
    fn edges(&self) -> Vec<Vec<(usize, Option<Known>)>> {
        let mut into = vec![Vec::new(); self.body.basic_blocks.len()];
        for (block, data) in self.body.basic_blocks.iter().enumerate() {
            let Some(terminator) = &data.terminator else {
                continue;
            };
            let guarded = self.guard_of(block, terminator);
            for successor in terminator.successors() {
                if let Some(list) = into.get_mut(successor.0) {
                    let on_true = guarded.filter(|(target, _)| *target == successor.0);
                    list.push((block, on_true.map(|(_, guard)| guard)));
                }
            }
        }
        into
    }

    /// `(true target, (index, length, false))` when `terminator` switches on
    /// `index < length` computed in `block`, both kept to the switch, and the
    /// index is never negative. The fact is completed per collection later.
    fn guard_of(&self, block: usize, terminator: &Terminator) -> Option<(usize, Known)> {
        let TerminatorKind::SwitchInt { discr, targets, .. } = &terminator.kind else {
            return None;
        };
        let [(value, target)] = targets.as_slice() else {
            return None;
        };
        if *value != Discriminant::bool_true() {
            return None;
        }
        let condition = discr.bare_local()?;
        let statements = &self.body.basic_blocks.get(block)?.statements;
        let at = statements.iter().rposition(|s| assigns(s, condition))?;
        let (StatementKind::Assign(_, Rvalue::BinaryOp(BinOp::Lt, lhs, rhs))
        | StatementKind::Reassign(_, Rvalue::BinaryOp(BinOp::Lt, lhs, rhs))) =
            &statements.get(at)?.kind
        else {
            return None;
        };
        let (index, length) = (lhs.bare_local()?, rhs.bare_local()?);
        let kept = statements
            .get(at + 1..)
            .unwrap_or_default()
            .iter()
            .all(|s| !assigns(s, index) && !assigns(s, length));
        (kept && self.never_negative.contains(&index.0))
            .then_some((target.0, (index.0, length.0, false)))
    }

    /// The facts known on entry to each block, by repeated meets until none
    /// changes; `None` for a block not yet reached.
    fn entry_states(&self) -> Vec<Option<HashSet<Known>>> {
        let count = self.body.basic_blocks.len();
        let mut entries: Vec<Option<HashSet<Known>>> = vec![None; count];
        if count == 0 {
            return entries;
        }
        let mut changed = true;
        while changed {
            changed = false;
            for block in 0..count {
                let next = self.meet(block, &entries);
                if next.is_some() && next != entries[block] {
                    entries[block] = next;
                    changed = true;
                }
            }
        }
        entries
    }

    fn meet(&self, block: usize, entries: &[Option<HashSet<Known>>]) -> Option<HashSet<Known>> {
        let mut result: Option<HashSet<Known>> = (block == 0).then(HashSet::new);
        for (pred, guard) in self.predecessors.get(block).into_iter().flatten() {
            let Some(mut facts) = entries.get(*pred).cloned().flatten() else {
                continue;
            };
            self.run_block(*pred, &mut facts);
            if let Some((index, length, _)) = guard {
                let proven: Vec<Known> = (facts.iter())
                    .filter(|(local, _, is_index)| !is_index && local == length)
                    .map(|(_, collection, _)| (*index, *collection, true))
                    .collect();
                facts.extend(proven);
            }
            result = Some(match result {
                None => facts,
                Some(so_far) => so_far.intersection(&facts).copied().collect(),
            });
        }
        result
    }

    /// `facts` after every statement and the terminator of `block`.
    fn run_block(&self, block: usize, facts: &mut HashSet<Known>) {
        let Some(data) = self.body.basic_blocks.get(block) else {
            return;
        };
        for statement in &data.statements {
            self.step(statement, facts);
        }
        if let Some(terminator) = &data.terminator {
            step_terminator(self.body, terminator, facts);
        }
    }

    fn step(&self, statement: &Statement, facts: &mut HashSet<Known>) {
        match &statement.kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue)
                if place.projection.is_empty() =>
            {
                let dest = place.local.0;
                let made = self.made_by(dest, rvalue, facts);
                facts.retain(|(local, collection, _)| *local != dest && *collection != dest);
                facts.extend(made);
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                let gone = place.local.0;
                facts.retain(|(local, collection, _)| *local != gone && *collection != gone);
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place)
                if place.projection.is_empty() =>
            {
                facts.retain(|(_, collection, _)| *collection != place.local.0);
            }
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::IncRef(_)
            | StatementKind::Nop => {}
        }
    }

    fn made_by(&self, dest: usize, rvalue: &Rvalue, facts: &HashSet<Known>) -> Vec<Known> {
        match rvalue {
            Rvalue::Len(measured) if measured.projection.is_empty() => {
                let is_indexed = self
                    .body
                    .local_decls
                    .get(measured.local.0)
                    .is_some_and(|d| {
                        matches!(
                            d.ty.kind.as_builtin_collection(),
                            Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                        )
                    });
                (is_indexed && measured.local.0 != dest)
                    .then_some((dest, measured.local.0, false))
                    .into_iter()
                    .collect()
            }
            Rvalue::Use(operand) => operand
                .bare_local()
                .filter(|source| source.0 != dest)
                .map(|source| restated(facts, source.0, dest))
                .unwrap_or_default(),
            Rvalue::Len(_)
            | Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::Aggregate(..)
            | Rvalue::Phi(_)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        }
    }

    /// Why `proof` does not hold, if it does not.
    fn failure(&self, proof: &InBounds, entries: &[Option<HashSet<Known>>]) -> Option<String> {
        let block = proof.block.0;
        let Some(data) = self.body.basic_blocks.get(block) else {
            return Some("its block does not exist".to_string());
        };
        let covered =
            self.body.stable_collections.iter().any(|record| {
                record.local == proof.collection && record.blocks.contains(&proof.block)
            });
        if !covered {
            return Some(format!(
                "no stable record of `{}` covers it",
                proof.collection
            ));
        }
        if !indexes_at(data, proof) {
            return Some("nothing there indexes it".to_string());
        }
        let Some(mut facts) = entries.get(block).cloned().flatten() else {
            return Some("its block is never reached".to_string());
        };
        for statement in data.statements.iter().take(proof.statement_index) {
            self.step(statement, &mut facts);
        }
        let known = (proof.index.0, proof.collection.0, true);
        (!facts.contains(&known)).then(|| "no guard proves it there".to_string())
    }
}

/// The facts mentioning `from`, restated of `to`.
fn restated(facts: &HashSet<Known>, from: usize, to: usize) -> Vec<Known> {
    facts
        .iter()
        .filter(|(local, collection, _)| *local == from || *collection == from)
        .map(|(local, collection, is_index)| {
            let swap = |l: usize| if l == from { to } else { l };
            (swap(*local), swap(*collection), *is_index)
        })
        .collect()
}

fn step_terminator(body: &Body, terminator: &Terminator, facts: &mut HashSet<Known>) {
    match &terminator.kind {
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            let name = func.called_runtime_name();
            let keeps_length = name.is_some_and(preserves_length);
            let source = args
                .first()
                .and_then(Operand::bare_local)
                .filter(|_| keeps_length);
            if !name.is_some_and(|name| runtime_call_leaves_headers(body, name, args)) {
                facts.clear();
            }
            if destination.projection.is_empty() {
                let dest = destination.local.0;
                let made = source
                    .map(|s| restated(facts, s.0, dest))
                    .unwrap_or_default();
                facts.retain(|(local, collection, _)| *local != dest && *collection != dest);
                facts.extend(made);
            }
        }
        TerminatorKind::VirtualCall { .. } | TerminatorKind::GpuLaunch { .. } => facts.clear(),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => {}
    }
}

/// Whether the statement or terminator `proof` names indexes its collection
/// by its index at the first projection.
fn indexes_at(data: &crate::mir::BasicBlockData, proof: &InBounds) -> bool {
    let mut found = FindsIndex {
        collection: proof.collection,
        index: proof.index,
        found: false,
    };
    match data.statements.get(proof.statement_index) {
        Some(statement) => found.visit_statement(proof.block, statement),
        None if proof.statement_index == data.statements.len() => {
            if let Some(terminator) = &data.terminator {
                found.visit_terminator(proof.block, terminator);
            }
        }
        None => {}
    }
    found.found
}

struct FindsIndex {
    collection: Local,
    index: Local,
    found: bool,
}

impl Visitor for FindsIndex {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if place.local == self.collection
            && place.projection.first() == Some(&PlaceElem::Index(self.index))
        {
            self.found = true;
        }
    }
}

/// Whether `statement` gives the whole of `local` a value or bounds its
/// storage.
fn assigns(statement: &Statement, local: Local) -> bool {
    match &statement.kind {
        StatementKind::Assign(place, _)
        | StatementKind::Reassign(place, _)
        | StatementKind::StorageLive(place)
        | StatementKind::StorageDead(place) => place.local == local && place.projection.is_empty(),
        StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => false,
    }
}

/// The locals no definition makes negative: unsigned ones, and signed
/// locals of the body's own — no parameter, capture, call result, projected
/// write or address taken — each of whose definitions is a non-negative
/// constant, a length, a copy of such a local, or a sum or product of such
/// operands, which overflow-checked arithmetic keeps non-negative.
fn never_negative(body: &Body) -> HashSet<usize> {
    let (definitions, foreign) = own_definitions(body);
    let mut known: HashSet<usize> = (0..body.local_decls.len())
        .filter(|l| is_signed(body, *l) && !foreign.contains(l) && definitions.contains_key(l))
        .collect();
    loop {
        let current = known.clone();
        known.retain(|local| {
            (definitions.get(local).into_iter().flatten())
                .all(|rvalue| keeps_non_negative(body, rvalue, &current))
        });
        if known.len() == current.len() {
            break;
        }
    }
    known.extend((0..body.local_decls.len()).filter(|l| is_unsigned(body, *l)));
    known
}

/// Every rvalue each whole local is given, and the locals set some other
/// way: parameters, captures, call results, projected writes and locals
/// whose address is taken.
fn own_definitions(body: &Body) -> (HashMap<usize, Vec<&Rvalue>>, HashSet<usize>) {
    let mut definitions: HashMap<usize, Vec<&Rvalue>> = HashMap::new();
    let mut foreign: HashSet<usize> = (1..=body.arg_count).collect();
    foreign.extend(body.env_capture_locals.iter().map(|l| l.0));
    for data in &body.basic_blocks {
        for statement in &data.statements {
            if let StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) =
                &statement.kind
            {
                if place.projection.is_empty() {
                    definitions.entry(place.local.0).or_default().push(rvalue);
                } else {
                    foreign.insert(place.local.0);
                }
                if let Rvalue::Ref(taken) = rvalue {
                    foreign.insert(taken.local.0);
                }
            }
        }
        let written = data.terminator.iter().flat_map(Terminator::written_places);
        foreign.extend(written.map(|p| p.local.0));
    }
    (definitions, foreign)
}

/// Whether `rvalue` is never negative when every local of `known` is not.
fn keeps_non_negative(body: &Body, rvalue: &Rvalue, known: &HashSet<usize>) -> bool {
    let operand_ok = |operand: &Operand| match operand {
        Operand::Constant(constant) => {
            matches!(&constant.literal, Literal::Integer(value) if literal_non_negative(value))
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Function(_) => operand
            .bare_local()
            .is_some_and(|l| known.contains(&l.0) || is_unsigned(body, l.0)),
    };
    match rvalue {
        Rvalue::Use(operand) => operand_ok(operand),
        Rvalue::Len(_) => true,
        Rvalue::BinaryOp(BinOp::Add | BinOp::Mul, lhs, rhs) => {
            crate::mir::optimization::in_bounds::INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW
                && operand_ok(lhs)
                && operand_ok(rhs)
        }
        Rvalue::BinaryOp(..)
        | Rvalue::Ref(_)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::Aggregate(..)
        | Rvalue::Phi(_)
        | Rvalue::AtomicOp { .. } => false,
    }
}

fn is_unsigned(body: &Body, local: usize) -> bool {
    body.local_decls.get(local).is_some_and(|d| {
        matches!(
            d.ty.kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        )
    })
}

fn is_signed(body: &Body, local: usize) -> bool {
    body.local_decls.get(local).is_some_and(|d| {
        matches!(
            d.ty.kind,
            TypeKind::Int
                | TypeKind::I8
                | TypeKind::I16
                | TypeKind::I32
                | TypeKind::I64
                | TypeKind::I128
        )
    })
}

fn literal_non_negative(value: &IntegerLiteral) -> bool {
    match value {
        IntegerLiteral::I8(v) => *v >= 0,
        IntegerLiteral::I16(v) => *v >= 0,
        IntegerLiteral::I32(v) => *v >= 0,
        IntegerLiteral::I64(v) => *v >= 0,
        IntegerLiteral::I128(v) => *v >= 0,
        IntegerLiteral::U8(_)
        | IntegerLiteral::U16(_)
        | IntegerLiteral::U32(_)
        | IntegerLiteral::U64(_)
        | IntegerLiteral::U128(_) => true,
    }
}
