// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finds the element accesses a loop's guard already proves in bounds, so
//! code generation emits them without comparing the index to the length.
//!
//! A forward must-dataflow carries three facts from block to block:
//!
//! - `n` holds the length of collection `l` — made by `n = Len(l)`, carried
//!   to the result of a copy-on-write or a clone of `l`, which keep its
//!   length (`runtime_fns::preserves_length`), and to a whole-local copy;
//!   lost when `n` or `l` is given another value, their storage ends, `l` is
//!   released, or a call runs that
//!   `analysis::header_reach::runtime_call_leaves_headers` does not clear;
//! - `a + m <= b` for a margin `m >= 0` (`a < b` is a margin of 1, `a <= b`
//!   one of 0), between integers of one type — made on the edge a
//!   `switchInt(c)` takes when `c = Lt(a, b)` or `c = Le(a, b)` holds (or `c`
//!   is a whole copy of such a comparison made in the same block), if the
//!   comparison's operands keep their values to the switch; that edge also
//!   carries the order one step on, `a + m + m' <= x` for each
//!   `b + m' <= x` known there. `x = y + k` and `x = y - k`, for a constant
//!   `k >= 0`, carry `y`'s margins to `x`: less `k` for a sum, the same for a
//!   difference, which also makes `x + k <= y`. Lost when either side is
//!   given another value;
//! - `i` is in bounds of `l` — made wherever `i` gains a margin (on that
//!   edge, or when `i` is computed by such a sum or difference) when `i` is
//!   at least 0 and `i + m <= L` with `m >= 1` for a local `L` holding the
//!   length of `l`; also on the true edge of `i < n` with `n` the length of
//!   `l` and `i` never negative. `i` is at least 0 when it is never negative,
//!   or when `a + m <= i` for an `a` that is never negative. Lost when `i`
//!   is given another value or anything that loses `l`'s length runs.
//!
//! Every margin is a statement about mathematical integers, and the
//! arithmetic it follows stops the program rather than wrap
//! ([`INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW`]), so `x = y + k` is exactly that
//! sum. The off-by-one cases: `i <= n <= L` admits `i == L`, a margin of 0;
//! `i < n` with `n <= L` gives `i + 1 <= L`, so `i + 1` itself needs
//! `n + 1 <= L`; and `lo < hi` with `hi + 1 <= L`, `lo` never negative,
//! keeps both `lo` and `hi` inside, `hi` stepping down keeping its margin.
//! The loop-versioning pass puts the guards these need in front of a loop.
//!
//! Whether `i` is never negative is decided once for the whole body
//! (`analysis::non_negative`): every definition of it is a non-negative
//! constant, a length, a copy of such a local, or a sum or product of such
//! operands, which overflow-checked arithmetic keeps non-negative
//! ([`INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW`]).
//!
//! An access is recorded on [`Body::in_bounds`] when its index is the first
//! projection of a place rooted at a collection a stable-collection record
//! covers in that block: code generation reads that collection's length once
//! before the loop, and it is that length the guard compared against. The
//! pass runs last among the MIR passes; any pass that later changes a block
//! must drop the entries in it, and `--verify-mir` re-derives every entry.

use std::collections::{BTreeSet, HashSet};

use crate::ast::types::BuiltinCollectionKind;
use crate::mir::analysis::header_reach::runtime_call_leaves_headers;
use crate::mir::analysis::loops::{comparison_before_switch, Comparison};
use crate::mir::analysis::non_negative::{constant_step, is_integer, non_negative_locals, Step};
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BinOp, Body, Discriminant, InBounds, Local, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::preserves_length;

/// Whether integer `+`, `-` and `*` stop the program on overflow rather than
/// wrap; the non-negativity rule relies on it.
pub use crate::mir::analysis::non_negative::INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW;

/// What the dataflow knows at a program point, by local index.
///
/// Every fact speaks of the values its locals hold at that point. A write to
/// a local kills every fact mentioning it before it derives any about the new
/// value, and derives those only from facts whose other locals keep their
/// values across the write, so no fact about an old value is ever restated
/// of a new one; and no order relates a local to itself ([`Fact::below`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fact {
    /// `length` holds the number of elements of `collection`.
    LengthOf { length: usize, collection: usize },
    /// `index` is at least 0 and below the length of `collection`.
    InBounds { index: usize, collection: usize },
    /// `lhs + margin <= rhs`: `a < b` is a margin of 1, `a <= b` one of 0.
    /// Both are integers of one type, and the sum is of mathematical
    /// integers, so no width bounds it.
    Below { lhs: usize, rhs: usize, margin: u64 },
}

impl Fact {
    /// `lhs + margin <= rhs`, unless `lhs` and `rhs` are one local: no
    /// write makes `x + m <= x` true of a value, so such a fact could only
    /// come from mixing an old value of `x` with its new one.
    fn below(lhs: usize, rhs: usize, margin: u64) -> Option<Fact> {
        (lhs != rhs).then_some(Fact::Below { lhs, rhs, margin })
    }

    fn mentions(self, local: usize) -> bool {
        match self {
            Fact::LengthOf { length, collection } => length == local || collection == local,
            Fact::InBounds { index, collection } => index == local || collection == local,
            Fact::Below { lhs, rhs, .. } => lhs == local || rhs == local,
        }
    }

    fn mentions_collection(self, local: usize) -> bool {
        match self {
            Fact::LengthOf { collection, .. } | Fact::InBounds { collection, .. } => {
                collection == local
            }
            Fact::Below { .. } => false,
        }
    }

    /// The fact restated of `to`, given `from`'s value by a whole copy, if
    /// it mentions `from` and not `to`: a fact mentioning `to` speaks of the
    /// value the copy replaces.
    fn renamed(self, from: usize, to: usize) -> Option<Fact> {
        if !self.mentions(from) || self.mentions(to) {
            return None;
        }
        let rename = |local: usize| if local == from { to } else { local };
        match self {
            Fact::LengthOf { length, collection } => Some(Fact::LengthOf {
                length: rename(length),
                collection: rename(collection),
            }),
            Fact::InBounds { index, collection } => Some(Fact::InBounds {
                index: rename(index),
                collection: rename(collection),
            }),
            Fact::Below { lhs, rhs, margin } => Fact::below(rename(lhs), rename(rhs), margin),
        }
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
            Rvalue::BinaryOp(op, lhs, rhs) => self.offset_facts(dest, *op, lhs, rhs, state),
            Rvalue::Use(_)
            | Rvalue::Ref(_)
            | Rvalue::UnaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::Aggregate(..)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        };
        state.retain(|fact| !fact.mentions(dest.0));
        state.extend(made);
        if matches!(rvalue, Rvalue::BinaryOp(..)) {
            let in_bounds = self.in_bounds_of(state, dest.0);
            state.extend(in_bounds);
        }
    }

    /// The margins `dest = source + value` or `dest = source - value` keeps,
    /// for a constant `value >= 0`. Integer `+` and `-` stop the program on
    /// overflow ([`INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW`]), so `dest` is
    /// exactly that sum or difference: where `source + m <= b`, a sum gives
    /// `dest + (m - value) <= b` when `m >= value`, and a difference, at most
    /// `source`, gives `dest + m <= b` and `dest + value <= source`. Only
    /// facts whose `b` is not `dest` are read, so each one made relates the
    /// new `dest` to a local the write leaves alone; for `x = x + k` the
    /// facts read are those of the old `x`, which is exactly the `source`.
    fn offset_facts(
        &self,
        dest: Local,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
        state: &Facts,
    ) -> Vec<Fact> {
        let Some((source, step)) = constant_step(op, lhs, rhs) else {
            return Vec::new();
        };
        if !self.are_integers_of_one_type(dest, source) {
            return Vec::new();
        }
        let mut made: Vec<Fact> = (state.iter())
            .filter_map(|fact| match *fact {
                Fact::Below { lhs, rhs, margin } if lhs == source.0 && rhs != dest.0 => {
                    let margin = match step {
                        Step::Up(value) => margin.checked_sub(value)?,
                        Step::Down(_) => margin,
                    };
                    Fact::below(dest.0, rhs, margin)
                }
                Fact::Below { .. } | Fact::LengthOf { .. } | Fact::InBounds { .. } => None,
            })
            .collect();
        if let Step::Down(value) = step {
            made.extend(Fact::below(dest.0, source.0, value));
        }
        made
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
                let mut edges: Vec<(usize, Facts)> = targets
                    .iter()
                    .map(|(value, target)| {
                        let mut facts = state.clone();
                        if *value == Discriminant::bool_true() {
                            self.guard(block, discr, &mut facts);
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

    /// Adds to `facts` what the true edge of a switch on `discr`, ending
    /// `block`, makes: the order its comparison `lhs < rhs` or `lhs <= rhs`
    /// states, that order carried one step past each local `rhs` is known
    /// below, and the collections either operand then lies inside.
    fn guard(&self, block: usize, discr: &Operand, facts: &mut Facts) {
        let Some(condition) = discr.bare_local() else {
            return;
        };
        let Some(Comparison { lhs, rhs, strict }) =
            comparison_before_switch(self.body, block, condition)
        else {
            return;
        };
        if strict && self.is_non_negative(lhs) {
            let below_its_length = lengths_held_by(facts, rhs.0)
                .map(|collection| Fact::InBounds {
                    index: lhs.0,
                    collection,
                })
                .collect::<Vec<_>>();
            facts.extend(below_its_length);
        }
        if lhs == rhs || !self.are_integers_of_one_type(lhs, rhs) {
            return;
        }
        let margin = u64::from(strict);
        let carried: Vec<Fact> = (facts.iter())
            .filter_map(|fact| match *fact {
                Fact::Below {
                    lhs: below,
                    rhs: above,
                    margin: further,
                } if below == rhs.0 => Fact::below(lhs.0, above, margin.checked_add(further)?),
                Fact::Below { .. } | Fact::LengthOf { .. } | Fact::InBounds { .. } => None,
            })
            .collect();
        facts.extend(Fact::below(lhs.0, rhs.0, margin));
        facts.extend(carried);
        for operand in [lhs, rhs] {
            let in_bounds = self.in_bounds_of(facts, operand.0);
            facts.extend(in_bounds);
        }
    }

    /// The collections `facts` keeps `index` inside: `index` is at least 0 —
    /// never negative anywhere, or at least a local that never is — and
    /// below a local holding the collection's length by a margin of 1 or
    /// more.
    fn in_bounds_of(&self, facts: &Facts, index: usize) -> Vec<Fact> {
        let at_least_zero = self.non_negative.contains(&index)
            || facts.iter().any(|fact| match *fact {
                Fact::Below { lhs, rhs, .. } => rhs == index && self.non_negative.contains(&lhs),
                Fact::LengthOf { .. } | Fact::InBounds { .. } => false,
            });
        if !at_least_zero {
            return Vec::new();
        }
        let below = |length: usize| {
            facts.iter().any(|fact| {
                matches!(*fact, Fact::Below { lhs, rhs, margin }
                    if lhs == index && rhs == length && margin >= 1)
            })
        };
        (facts.iter())
            .filter_map(|fact| match *fact {
                Fact::LengthOf { length, collection } if below(length) => {
                    Some(Fact::InBounds { index, collection })
                }
                Fact::LengthOf { .. } | Fact::InBounds { .. } | Fact::Below { .. } => None,
            })
            .collect()
    }

    /// Whether `lhs` and `rhs` are declared the same integer type, so one
    /// ordering of them carries to another.
    fn are_integers_of_one_type(&self, lhs: Local, rhs: Local) -> bool {
        let kind = |local: Local| self.body.local_decls.get(local.0).map(|decl| &decl.ty.kind);
        kind(lhs).is_some_and(|left| is_integer(left) && kind(rhs) == Some(left))
    }

    fn is_non_negative(&self, local: Local) -> bool {
        self.non_negative.contains(&local.0)
    }
}

/// The collections `bound` holds the length of in `facts`.
fn lengths_held_by(facts: &Facts, bound: usize) -> impl Iterator<Item = usize> + '_ {
    facts.iter().filter_map(move |fact| match *fact {
        Fact::LengthOf { length, collection } if length == bound => Some(collection),
        Fact::LengthOf { .. } | Fact::InBounds { .. } | Fact::Below { .. } => None,
    })
}

/// The facts of `state` mentioning `from`, restated of `to`.
fn renamed_facts(state: &Facts, from: Local, to: Local) -> Vec<Fact> {
    state
        .iter()
        .filter_map(|fact| fact.renamed(from.0, to.0))
        .collect()
}
