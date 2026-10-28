// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every access a body records as in bounds is proven so.
//!
//! Code generation emits a recorded access without comparing its index to
//! the collection's length, so a record nothing proves reads or writes past
//! the end. Each entry is re-derived here from the body alone, with a
//! dataflow of its own over three kinds of fact:
//!
//! - a length is known of a collection from `Len` until the length local or
//!   the collection is given another value (a copy-on-write or clone of the
//!   collection, or a whole-local copy, carries it), its storage ends, or a
//!   call outside the header-preserving runtime entry points runs;
//! - an order `a + m <= b`, `m >= 0`, between integers of one type, is known
//!   on the taken edge of a switch on `a < b` (`m = 1`) or `a <= b` (`m = 0`)
//!   computed in the switch's block, directly or into a local the switched
//!   one is a whole copy of, its operands kept to the switch; the same edge
//!   knows `a + m + m' <= c` for each `b + m' <= c` known before it. A sum
//!   `x = y + k` or difference `x = y - k`, `k` a constant at least 0, knows
//!   `x + (m - k) <= c` (sum, `m >= k`) or `x + m <= c` and `x + k <= y`
//!   (difference) from each `y + m <= c`: the arithmetic traps rather than
//!   wraps, so `x` is exactly that value;
//! - an index is known in bounds of a collection once it is at least 0 —
//!   never negative, or ordered above a never-negative local — and ordered
//!   below a local holding the length by a margin of 1 or more, checked
//!   where an order about it is made; or on the taken edge of `i < n`, `n`
//!   the length and `i` never negative.
//!
//! The guard of a switch holds only along its true edge; an `otherwise`
//! leading to the same block does not carry it. The access must also sit
//! where the entry says and lie in a block a stable-collection record of the
//! collection covers, since it is that record's length code generation
//! compares against no more.

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

/// What is known of the locals at a program point, by local index. Each
/// fact is of the values the locals hold there: a write kills every fact
/// naming its destination, and what it adds is derived only from facts
/// whose other locals the write leaves alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Known {
    /// `length` holds the length of `collection`.
    Length { length: usize, collection: usize },
    /// `index` lies in bounds of `collection`.
    Index { index: usize, collection: usize },
    /// `lhs + margin <= rhs` over mathematical integers: `<` is a margin
    /// of 1, `<=` one of 0.
    Order { lhs: usize, rhs: usize, margin: u64 },
}

impl Known {
    /// Whether the fact says anything of `local`.
    fn names(self, local: usize) -> bool {
        match self {
            Known::Length {
                length: a,
                collection: b,
            }
            | Known::Index {
                index: a,
                collection: b,
            }
            | Known::Order { lhs: a, rhs: b, .. } => a == local || b == local,
        }
    }

    /// Whether the fact says anything of the collection `local`.
    fn names_collection(self, local: usize) -> bool {
        match self {
            Known::Length { collection, .. } | Known::Index { collection, .. } => {
                collection == local
            }
            Known::Order { .. } => false,
        }
    }

    /// The fact with every mention of `from` made of `to`, for `to` taking
    /// `from`'s value; `None` when the fact also names `to`, whose value it
    /// speaks of is the one being replaced.
    fn restated(self, from: usize, to: usize) -> Option<Known> {
        if self.names(to) {
            return None;
        }
        let swap = |local: usize| if local == from { to } else { local };
        match self {
            Known::Length { length, collection } => Some(Known::Length {
                length: swap(length),
                collection: swap(collection),
            }),
            Known::Index { index, collection } => Some(Known::Index {
                index: swap(index),
                collection: swap(collection),
            }),
            Known::Order { lhs, rhs, margin } => order(swap(lhs), swap(rhs), margin),
        }
    }
}

/// A switch's comparison `lhs < rhs` (`strict`) or `lhs <= rhs`, which holds
/// on the edge it takes when true.
#[derive(Debug, Clone, Copy)]
struct Guard {
    lhs: usize,
    rhs: usize,
    strict: bool,
}

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
    predecessors: Vec<Vec<(usize, Option<Guard>)>>,
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

    /// For each block, every edge into it with the comparison known true
    /// along it.
    fn edges(&self) -> Vec<Vec<(usize, Option<Guard>)>> {
        let mut into = vec![Vec::new(); self.body.basic_blocks.len()];
        for (block, data) in self.body.basic_blocks.iter().enumerate() {
            let Some(terminator) = &data.terminator else {
                continue;
            };
            let guarded = self.guard_of(block, terminator);
            // A guarded switch has one target, its true edge, which
            // `successors` lists first; `otherwise` follows it, and may be
            // the same block without the guard holding along it.
            for (position, successor) in terminator.successors().into_iter().enumerate() {
                if let Some(list) = into.get_mut(successor.0) {
                    list.push((block, guarded.filter(|_| position == 0)));
                }
            }
        }
        into
    }

    /// The guard of the true edge when `terminator` switches on `lhs < rhs`
    /// or `lhs <= rhs` computed in `block` — directly, or into a local the
    /// switched one is a whole copy of — both operands kept to the switch.
    fn guard_of(&self, block: usize, terminator: &Terminator) -> Option<Guard> {
        let TerminatorKind::SwitchInt { discr, targets, .. } = &terminator.kind else {
            return None;
        };
        let [(value, _)] = targets.as_slice() else {
            return None;
        };
        if *value != Discriminant::bool_true() {
            return None;
        }
        let condition = discr.bare_local()?;
        let statements = &self.body.basic_blocks.get(block)?.statements;
        let mut at = statements.iter().rposition(|s| assigns(s, condition))?;
        if let Some(source) = whole_copy_of(statements.get(at)?) {
            at = statements
                .get(..at)?
                .iter()
                .rposition(|s| assigns(s, source))?;
        }
        let (StatementKind::Assign(_, Rvalue::BinaryOp(op, lhs, rhs))
        | StatementKind::Reassign(_, Rvalue::BinaryOp(op, lhs, rhs))) = &statements.get(at)?.kind
        else {
            return None;
        };
        let strict = is_strict_order(*op)?;
        let (lhs, rhs) = (lhs.bare_local()?, rhs.bare_local()?);
        let kept = statements
            .get(at + 1..)
            .unwrap_or_default()
            .iter()
            .all(|s| !assigns(s, lhs) && !assigns(s, rhs));
        kept.then_some(Guard {
            lhs: lhs.0,
            rhs: rhs.0,
            strict,
        })
    }

    /// What `guard` adds to `facts`: `lhs` in bounds of a collection whose
    /// length `rhs` holds, for a strict guard and a never-negative `lhs`;
    /// and, between integers of one type, the order it states, that order
    /// carried past each local `rhs` is ordered below, and each collection
    /// either operand then lies inside.
    fn proven_by(&self, guard: Guard, facts: &HashSet<Known>) -> Vec<Known> {
        let mut proven: Vec<Known> = facts
            .iter()
            .filter_map(|fact| match *fact {
                Known::Length { length, collection }
                    if guard.strict
                        && length == guard.rhs
                        && self.never_negative.contains(&guard.lhs) =>
                {
                    Some(Known::Index {
                        index: guard.lhs,
                        collection,
                    })
                }
                Known::Length { .. } | Known::Index { .. } | Known::Order { .. } => None,
            })
            .collect();
        if guard.lhs == guard.rhs || !same_integer_type(self.body, guard.lhs, guard.rhs) {
            return proven;
        }
        let margin = u64::from(guard.strict);
        proven.extend(order(guard.lhs, guard.rhs, margin));
        proven.extend(facts.iter().filter_map(|fact| match *fact {
            Known::Order {
                lhs,
                rhs,
                margin: more,
            } if lhs == guard.rhs => order(guard.lhs, rhs, margin.checked_add(more)?),
            Known::Order { .. } | Known::Length { .. } | Known::Index { .. } => None,
        }));
        let mut after = facts.clone();
        after.extend(proven.iter().copied());
        for operand in [guard.lhs, guard.rhs] {
            proven.extend(self.inside(&after, operand));
        }
        proven
    }

    /// `Index` facts for every collection `facts` puts `local` inside: it is
    /// at least 0 — never negative, or ordered above a never-negative local
    /// — and ordered at least 1 below a local holding the length.
    fn inside(&self, facts: &HashSet<Known>, local: usize) -> Vec<Known> {
        let is_never_negative = |l: usize| self.never_negative.contains(&l);
        let at_least_zero = is_never_negative(local)
            || facts.iter().any(|fact| {
                matches!(*fact, Known::Order { lhs, rhs, .. } if rhs == local && is_never_negative(lhs))
            });
        if !at_least_zero {
            return Vec::new();
        }
        facts
            .iter()
            .filter_map(|fact| match *fact {
                Known::Length { length, collection } if is_below(facts, local, length) => {
                    Some(Known::Index {
                        index: local,
                        collection,
                    })
                }
                Known::Length { .. } | Known::Index { .. } | Known::Order { .. } => None,
            })
            .collect()
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
            if let Some(guard) = guard {
                let proven = self.proven_by(*guard, &facts);
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
                facts.retain(|fact| !fact.names(dest));
                facts.extend(made);
                if let Rvalue::BinaryOp(..) = rvalue {
                    let inside = self.inside(facts, dest);
                    facts.extend(inside);
                }
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                facts.retain(|fact| !fact.names(place.local.0));
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place)
                if place.projection.is_empty() =>
            {
                facts.retain(|fact| !fact.names_collection(place.local.0));
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
                    .then_some(Known::Length {
                        length: dest,
                        collection: measured.local.0,
                    })
                    .into_iter()
                    .collect()
            }
            Rvalue::Use(operand) => operand
                .bare_local()
                .filter(|source| source.0 != dest)
                .map(|source| restated(facts, source.0, dest))
                .unwrap_or_default(),
            Rvalue::BinaryOp(op, lhs, rhs) => self.offsets(dest, *op, lhs, rhs, facts),
            Rvalue::Len(_)
            | Rvalue::Ref(_)
            | Rvalue::UnaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::Aggregate(..)
            | Rvalue::Phi(_)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        }
    }

    /// The orders `dest = base + k`, `dest = k + base` or `dest = base - k`
    /// leaves, for an integer constant `k >= 0` and `dest` of `base`'s type.
    /// The arithmetic traps rather than wraps, so `dest` is exactly that
    /// value: `base + m <= r` with `m >= k` gives `dest + (m - k) <= r` for
    /// the sum, and `dest + m <= r` and `dest + k <= base` for the
    /// difference, which is at most `base`.
    fn offsets(
        &self,
        dest: usize,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
        facts: &HashSet<Known>,
    ) -> Vec<Known> {
        let Some(is_sum) = is_sum_or_difference(op) else {
            return Vec::new();
        };
        let (base, constant) = match (lhs.bare_local(), rhs.bare_local()) {
            (Some(base), None) => (base.0, rhs),
            (None, Some(base)) if is_sum => (base.0, lhs),
            (None, Some(_)) | (Some(_), Some(_)) | (None, None) => return Vec::new(),
        };
        let Some(k) = constant_at_least_zero(constant) else {
            return Vec::new();
        };
        if !same_integer_type(self.body, dest, base) {
            return Vec::new();
        }
        let mut made: Vec<Known> = facts
            .iter()
            .filter_map(|fact| match *fact {
                Known::Order { lhs, rhs, margin } if lhs == base && rhs != dest => {
                    let margin = if is_sum {
                        margin.checked_sub(k)?
                    } else {
                        margin
                    };
                    order(dest, rhs, margin)
                }
                Known::Order { .. } | Known::Length { .. } | Known::Index { .. } => None,
            })
            .collect();
        if !is_sum {
            made.extend(order(dest, base, k));
        }
        made
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
        let known = Known::Index {
            index: proof.index.0,
            collection: proof.collection.0,
        };
        (!facts.contains(&known)).then(|| "no guard proves it there".to_string())
    }
}

/// Whether `op` orders its operands strictly (`<`) or not (`<=`); `None`
/// for any other operator.
fn is_strict_order(op: BinOp) -> Option<bool> {
    match op {
        BinOp::Lt => Some(true),
        BinOp::Le => Some(false),
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::BitXor
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::Eq
        | BinOp::Ne
        | BinOp::Ge
        | BinOp::Gt
        | BinOp::Offset => None,
    }
}

/// The facts mentioning `from`, restated of `to`.
fn restated(facts: &HashSet<Known>, from: usize, to: usize) -> Vec<Known> {
    facts
        .iter()
        .filter(|fact| fact.names(from))
        .filter_map(|fact| fact.restated(from, to))
        .collect()
}

/// `lhs + margin <= rhs` as a fact, unless both are one local: such an
/// order could only come from an old value of a local mixed with its new
/// one, and is never made.
fn order(lhs: usize, rhs: usize, margin: u64) -> Option<Known> {
    (lhs != rhs).then_some(Known::Order { lhs, rhs, margin })
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
                facts.retain(|fact| !fact.names(dest));
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

/// Whether `lhs` and `rhs` are declared the same integer type.
fn same_integer_type(body: &Body, lhs: usize, rhs: usize) -> bool {
    let kind = |local: usize| body.local_decls.get(local).map(|d| &d.ty.kind);
    kind(lhs).is_some_and(|left| kind(rhs) == Some(left))
        && (is_signed(body, lhs) || is_unsigned(body, lhs))
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

/// Whether `facts` orders `local` below `length` by a margin of 1 or more.
fn is_below(facts: &HashSet<Known>, local: usize, length: usize) -> bool {
    facts.iter().any(|fact| {
        matches!(*fact, Known::Order { lhs, rhs, margin } if lhs == local && rhs == length && margin >= 1)
    })
}

/// Whether `op` adds (`true`) or subtracts (`false`); `None` for any other
/// operator.
fn is_sum_or_difference(op: BinOp) -> Option<bool> {
    match op {
        BinOp::Add => Some(true),
        BinOp::Sub => Some(false),
        BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::BitXor
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::Eq
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Ne
        | BinOp::Ge
        | BinOp::Gt
        | BinOp::Offset => None,
    }
}

/// The local `statement` copies whole into a whole local, if it does.
fn whole_copy_of(statement: &Statement) -> Option<Local> {
    match &statement.kind {
        StatementKind::Assign(_, Rvalue::Use(operand))
        | StatementKind::Reassign(_, Rvalue::Use(operand)) => operand.bare_local(),
        StatementKind::Assign(..)
        | StatementKind::Reassign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// The value of an integer constant `operand` that is at least 0.
fn constant_at_least_zero(operand: &Operand) -> Option<u64> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    let Literal::Integer(value) = &constant.literal else {
        return None;
    };
    u64::try_from(value.as_i128()?).ok()
}
