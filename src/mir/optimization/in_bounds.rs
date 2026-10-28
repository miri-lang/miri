// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finds the element accesses a loop's guard already proves in bounds, so
//! code generation emits them without comparing the index to the length.
//!
//! A forward must-dataflow carries two facts from block to block:
//!
//! - `n` holds the length of collection `l` — made by `n = Len(l)`, carried
//!   to the result of a copy-on-write or a clone of `l`, which keep its
//!   length (`runtime_fns::preserves_length`), and to a whole-local copy;
//!   lost when `n` or `l` is given another value, their storage ends, `l` is
//!   released, or a call runs that
//!   `analysis::header_reach::runtime_call_leaves_headers` does not clear;
//! - `i` is in bounds of `l` — made on the edge a `switchInt(c)` takes when
//!   `c = Lt(i, n)` holds, if `c`, `i` and `n` keep their values from the
//!   comparison to the switch, `n` holds the length of `l`, and `i` is never
//!   negative; lost when `i` is given another value or anything that loses
//!   `l`'s length runs.
//!
//! Only `<` proves anything: `i <= n` admits `i == n`, one past the end.
//!
//! Whether `i` is never negative is decided once for the whole body: every
//! definition of it is a non-negative constant, a length, a copy of such a
//! local, or a sum or product of such operands. A sum or product of
//! non-negative operands stays non-negative only because integer `+` and `*`
//! stop the program on overflow ([`INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW`]);
//! were they to wrap, a sum could turn negative, and this rule admits only
//! constants, lengths and copies. A parameter, a call's result, a local
//! written through a projection and one whose address is taken are never
//! known non-negative; an unsigned local always is.
//!
//! An access is recorded on [`Body::in_bounds`] when its index is the first
//! projection of a place rooted at a collection a stable-collection record
//! covers in that block: code generation reads that collection's length once
//! before the loop, and it is that length the guard compared against. The
//! pass runs last among the MIR passes; any pass that later changes a block
//! must drop the entries in it, and `--verify-mir` re-derives every entry.

use std::collections::{BTreeSet, HashMap, HashSet};

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

/// Whether integer `+`, `-` and `*` stop the program on overflow rather than
/// wrap. Code generation emits them overflow-checked at every optimization
/// level while this holds; the non-negativity rule above relies on it.
pub const INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW: bool = true;

/// What the dataflow knows at a program point, by local index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fact {
    /// `length` holds the number of elements of `collection`.
    LengthOf { length: usize, collection: usize },
    /// `index` is at least 0 and below the length of `collection`.
    InBounds { index: usize, collection: usize },
}

impl Fact {
    fn mentions(self, local: usize) -> bool {
        match self {
            Fact::LengthOf { length, collection } => length == local || collection == local,
            Fact::InBounds { index, collection } => index == local || collection == local,
        }
    }

    fn mentions_collection(self, local: usize) -> bool {
        match self {
            Fact::LengthOf { collection, .. } | Fact::InBounds { collection, .. } => {
                collection == local
            }
        }
    }

    /// The fact with `from` replaced by `to`, if it mentions `from`.
    fn renamed(self, from: usize, to: usize) -> Option<Fact> {
        let rename = |local: usize| if local == from { to } else { local };
        self.mentions(from).then(|| match self {
            Fact::LengthOf { length, collection } => Fact::LengthOf {
                length: rename(length),
                collection: rename(collection),
            },
            Fact::InBounds { index, collection } => Fact::InBounds {
                index: rename(index),
                collection: rename(collection),
            },
        })
    }
}

type Facts = BTreeSet<Fact>;

/// Records on [`Body::in_bounds`] every access of `body` its loop guard
/// proves in bounds. Returns true if any was recorded.
pub fn record_in_bounds(body: &mut Body) -> bool {
    body.in_bounds = if body.is_gpu() || body.stable_collections.is_empty() {
        Vec::new()
    } else {
        proven_accesses(body)
    };
    !body.in_bounds.is_empty()
}

/// How many statements each block of `body` holds, to tell afterwards which
/// blocks a later pass changed.
pub fn statement_counts(body: &Body) -> Vec<usize> {
    body.basic_blocks
        .iter()
        .map(|data| data.statements.len())
        .collect()
}

/// Drops every [`Body::in_bounds`] entry in a block whose statement count is
/// no longer `before`'s: a pass that inserted or removed statements there
/// moved the positions the entries name.
pub fn forget_changed_blocks(body: &mut Body, before: &[usize]) {
    let after = statement_counts(body);
    body.in_bounds
        .retain(|proof| before.get(proof.block.0) == after.get(proof.block.0));
}

/// Every access of `body` the dataflow proves in bounds and a stable record
/// covers.
fn proven_accesses(body: &Body) -> Vec<InBounds> {
    let flow = Flow::of(body);
    let entries = flow.solve();
    let mut found = Vec::new();
    for (index, data) in body.basic_blocks.iter().enumerate() {
        let Some(mut state) = entries.get(index).cloned().flatten() else {
            continue;
        };
        for (position, statement) in data.statements.iter().enumerate() {
            let mut roots = IndexRoots::default();
            roots.visit_statement(BasicBlock(index), statement);
            found.extend(flow.proven(index, position, &roots.roots, &state));
            flow.apply_statement(statement, &mut state);
        }
        if let Some(terminator) = &data.terminator {
            let mut roots = IndexRoots::default();
            roots.visit_terminator(BasicBlock(index), terminator);
            found.extend(flow.proven(index, data.statements.len(), &roots.roots, &state));
        }
    }
    found
}

/// The collection and index of every place indexed at its first projection.
#[derive(Default)]
struct IndexRoots {
    roots: Vec<(Local, Local)>,
}

impl Visitor for IndexRoots {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if let Some(PlaceElem::Index(index)) = place.projection.first() {
            self.roots.push((place.local, *index));
        }
    }
}

/// The body the dataflow runs over, with what it decides once for it.
struct Flow<'a> {
    body: &'a Body,
    non_negative: HashSet<usize>,
}

impl<'a> Flow<'a> {
    fn of(body: &'a Body) -> Self {
        Self {
            body,
            non_negative: non_negative_locals(body),
        }
    }

    /// The facts holding on entry to each block; `None` for a block no path
    /// reaches.
    fn solve(&self) -> Vec<Option<Facts>> {
        let count = self.body.basic_blocks.len();
        let mut entries: Vec<Option<Facts>> = vec![None; count];
        if count == 0 {
            return entries;
        }
        entries[0] = Some(Facts::new());
        let mut pending = vec![0];
        while let Some(block) = pending.pop() {
            let Some(mut state) = entries.get(block).cloned().flatten() else {
                continue;
            };
            let Some(data) = self.body.basic_blocks.get(block) else {
                continue;
            };
            for statement in &data.statements {
                self.apply_statement(statement, &mut state);
            }
            let Some(terminator) = &data.terminator else {
                continue;
            };
            for (successor, facts) in self.edges(block, terminator, state) {
                let Some(slot) = entries.get_mut(successor) else {
                    continue;
                };
                let merged = match slot {
                    None => facts,
                    Some(old) => old.intersection(&facts).copied().collect(),
                };
                if slot.as_ref() != Some(&merged) {
                    *slot = Some(merged);
                    pending.push(successor);
                }
            }
        }
        entries
    }

    /// The entries `roots`, accessed at `position` of `block`, earn in
    /// `state`.
    fn proven(
        &self,
        block: usize,
        position: usize,
        roots: &[(Local, Local)],
        state: &Facts,
    ) -> Vec<InBounds> {
        let mut seen = HashSet::new();
        roots
            .iter()
            .filter(|(collection, index)| {
                state.contains(&Fact::InBounds {
                    index: index.0,
                    collection: collection.0,
                }) && self.is_covered(block, *collection)
            })
            .filter(|root| seen.insert(**root))
            .map(|(collection, index)| InBounds {
                block: BasicBlock(block),
                statement_index: position,
                collection: *collection,
                index: *index,
            })
            .collect()
    }

    /// Whether a stable-collection record of `collection` covers `block`.
    fn is_covered(&self, block: usize, collection: Local) -> bool {
        self.body
            .stable_collections
            .iter()
            .any(|record| record.local == collection && record.blocks.contains(&BasicBlock(block)))
    }

    fn apply_statement(&self, statement: &Statement, state: &mut Facts) {
        match &statement.kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                if place.projection.is_empty() {
                    self.assign(place.local, rvalue, state);
                }
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                state.retain(|fact| !fact.mentions(place.local.0));
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                if place.projection.is_empty() {
                    state.retain(|fact| !fact.mentions_collection(place.local.0));
                }
            }
            StatementKind::IncRef(_) | StatementKind::Nop => {}
        }
    }

    /// `state` after the whole local `dest` is given `rvalue`.
    fn assign(&self, dest: Local, rvalue: &Rvalue, state: &mut Facts) {
        let made: Vec<Fact> = match rvalue {
            Rvalue::Len(measured)
                if measured.projection.is_empty()
                    && measured.local != dest
                    && self.is_indexed_collection(measured.local) =>
            {
                vec![Fact::LengthOf {
                    length: dest.0,
                    collection: measured.local.0,
                }]
            }
            Rvalue::Use(Operand::Copy(source) | Operand::Move(source))
                if source.projection.is_empty() && source.local != dest =>
            {
                renamed_facts(state, source.local, dest)
            }
            Rvalue::Use(_)
            | Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::Aggregate(..)
            | Rvalue::Phi(_)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        };
        state.retain(|fact| !fact.mentions(dest.0));
        state.extend(made);
    }

    fn is_indexed_collection(&self, local: Local) -> bool {
        self.body.local_decls.get(local.0).is_some_and(|decl| {
            matches!(
                decl.ty.kind.as_builtin_collection(),
                Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
            )
        })
    }

    /// Each successor of `terminator`, ending `block`, with the facts that
    /// hold on the edge to it.
    fn edges(
        &self,
        block: usize,
        terminator: &Terminator,
        mut state: Facts,
    ) -> Vec<(usize, Facts)> {
        match &terminator.kind {
            TerminatorKind::Goto { target } => vec![(target.0, state)],
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => {
                let guarded = self.guarded(block, discr, &state);
                let mut edges: Vec<(usize, Facts)> = targets
                    .iter()
                    .map(|(value, target)| {
                        let mut facts = state.clone();
                        if *value == Discriminant::bool_true() {
                            facts.extend(guarded.iter().copied());
                        }
                        (target.0, facts)
                    })
                    .collect();
                edges.push((otherwise.0, state));
                edges
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } => {
                self.call(func, args, destination, &mut state);
                target
                    .iter()
                    .map(|target| (target.0, state.clone()))
                    .collect()
            }
            TerminatorKind::VirtualCall { target, .. }
            | TerminatorKind::GpuLaunch { target, .. } => target
                .iter()
                .map(|target| (target.0, Facts::new()))
                .collect(),
            TerminatorKind::Return | TerminatorKind::Unreachable => Vec::new(),
        }
    }

    /// `state` after a call to `func` with `args` writing `destination`.
    fn call(&self, func: &Operand, args: &[Operand], destination: &Place, state: &mut Facts) {
        let name = func.called_runtime_name();
        let copied = if let [argument] = args {
            argument
                .place()
                .filter(|source| source.projection.is_empty() && name.is_some_and(preserves_length))
                .map(|source| source.local)
        } else {
            None
        };
        if !name.is_some_and(|name| runtime_call_leaves_headers(self.body, name, args)) {
            state.clear();
        }
        if !destination.projection.is_empty() {
            return;
        }
        let made = copied
            .filter(|source| *source != destination.local)
            .map(|source| renamed_facts(state, source, destination.local))
            .unwrap_or_default();
        state.retain(|fact| !fact.mentions(destination.local.0));
        state.extend(made);
    }

    /// The in-bounds facts the true edge of a switch on `discr`, ending
    /// `block`, proves in `state`.
    fn guarded(&self, block: usize, discr: &Operand, state: &Facts) -> Vec<Fact> {
        let Some(condition) = discr.bare_local() else {
            return Vec::new();
        };
        let Some((index, length)) = self.comparison_before_switch(block, condition) else {
            return Vec::new();
        };
        if !self.is_non_negative(index) {
            return Vec::new();
        }
        state
            .iter()
            .filter_map(|fact| match fact {
                Fact::LengthOf {
                    length: known,
                    collection,
                } if *known == length.0 => Some(Fact::InBounds {
                    index: index.0,
                    collection: *collection,
                }),
                Fact::LengthOf { .. } | Fact::InBounds { .. } => None,
            })
            .collect()
    }

    /// `(i, n)` when the last statement of `block` writing `condition` is
    /// `condition = Lt(i, n)` and nothing after it writes `condition`, `i` or
    /// `n`.
    fn comparison_before_switch(&self, block: usize, condition: Local) -> Option<(Local, Local)> {
        let statements = &self.body.basic_blocks.get(block)?.statements;
        let position = statements
            .iter()
            .rposition(|statement| writes_whole(statement, condition))?;
        let (StatementKind::Assign(_, Rvalue::BinaryOp(BinOp::Lt, lhs, rhs))
        | StatementKind::Reassign(_, Rvalue::BinaryOp(BinOp::Lt, lhs, rhs))) =
            &statements.get(position)?.kind
        else {
            return None;
        };
        let (index, length) = (lhs.bare_local()?, rhs.bare_local()?);
        let later = statements.get(position + 1..).unwrap_or_default();
        let is_kept = later
            .iter()
            .all(|statement| !writes_whole(statement, index) && !writes_whole(statement, length));
        is_kept.then_some((index, length))
    }

    fn is_non_negative(&self, local: Local) -> bool {
        self.non_negative.contains(&local.0)
    }
}

/// The facts of `state` mentioning `from`, restated of `to`.
fn renamed_facts(state: &Facts, from: Local, to: Local) -> Vec<Fact> {
    state
        .iter()
        .filter_map(|fact| fact.renamed(from.0, to.0))
        .collect()
}

/// Whether `statement` gives the whole of `local` a value or starts or ends
/// its storage.
fn writes_whole(statement: &Statement, local: Local) -> bool {
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

/// The integer locals of `body` no definition can make negative, decided
/// optimistically: every candidate starts known, and one with a definition
/// that could be negative given the others is dropped, until none is.
fn non_negative_locals(body: &Body) -> HashSet<usize> {
    let definitions = whole_definitions(body);
    let excluded = excluded_locals(body);
    let mut known: HashSet<usize> = (body.local_decls.iter().enumerate())
        .filter(|(index, decl)| {
            is_unsigned(&decl.ty.kind)
                || (is_signed_integer(&decl.ty.kind)
                    && !excluded.contains(index)
                    && definitions.contains_key(index))
        })
        .map(|(index, _)| index)
        .collect();
    loop {
        let dropped: Vec<usize> = (known.iter().copied())
            .filter(|local| !is_unsigned_local(body, *local))
            .filter(|local| {
                (definitions.get(local).into_iter().flatten())
                    .any(|rvalue| !is_non_negative_rvalue(rvalue, body, &known))
            })
            .collect();
        if dropped.is_empty() {
            return known;
        }
        for local in dropped {
            known.remove(&local);
        }
    }
}

/// Every rvalue each whole local is assigned, by local index.
fn whole_definitions(body: &Body) -> HashMap<usize, Vec<&Rvalue>> {
    let mut definitions: HashMap<usize, Vec<&Rvalue>> = HashMap::new();
    for statement in body.basic_blocks.iter().flat_map(|data| &data.statements) {
        if let StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) =
            &statement.kind
        {
            if place.projection.is_empty() {
                definitions.entry(place.local.0).or_default().push(rvalue);
            }
        }
    }
    definitions
}

/// The locals whose value code this body does not show may set: parameters,
/// captures, call results, locals written through a projection and locals
/// whose address is taken.
fn excluded_locals(body: &Body) -> HashSet<usize> {
    let mut excluded: HashSet<usize> = (1..=body.arg_count).collect();
    excluded.extend(body.env_capture_locals.iter().map(|local| local.0));
    for data in &body.basic_blocks {
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                    if !place.projection.is_empty() {
                        excluded.insert(place.local.0);
                    }
                    if let Rvalue::Ref(taken) = rvalue {
                        excluded.insert(taken.local.0);
                    }
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop => {}
            }
        }
        let written = data.terminator.iter().flat_map(Terminator::written_places);
        excluded.extend(written.map(|place| place.local.0));
    }
    excluded
}

/// Whether `rvalue` is non-negative when every local of `known` is.
fn is_non_negative_rvalue(rvalue: &Rvalue, body: &Body, known: &HashSet<usize>) -> bool {
    let operand_is = |operand: &Operand| is_non_negative_operand(operand, body, known);
    match rvalue {
        Rvalue::Use(operand) => operand_is(operand),
        Rvalue::Len(_) => true,
        Rvalue::BinaryOp(BinOp::Add | BinOp::Mul, lhs, rhs) => {
            INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW && operand_is(lhs) && operand_is(rhs)
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

fn is_non_negative_operand(operand: &Operand, body: &Body, known: &HashSet<usize>) -> bool {
    match operand {
        Operand::Constant(constant) => matches!(
            &constant.literal,
            Literal::Integer(value) if is_non_negative_literal(value)
        ),
        Operand::Copy(place) | Operand::Move(place) => {
            place.projection.is_empty()
                && (known.contains(&place.local.0) || is_unsigned_local(body, place.local.0))
        }
        Operand::Function(_) => false,
    }
}

fn is_non_negative_literal(value: &IntegerLiteral) -> bool {
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

fn is_unsigned_local(body: &Body, local: usize) -> bool {
    body.local_decls
        .get(local)
        .is_some_and(|decl| is_unsigned(&decl.ty.kind))
}

fn is_unsigned(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
    )
}

fn is_signed_integer(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
    )
}
