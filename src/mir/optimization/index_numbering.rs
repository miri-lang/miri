// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Names each computed index by the first temp that holds its value.
//!
//! Lowering computes an index written as an expression — `l[i + 1]` — into a
//! temp of its own at every use, so two writes to `l[i + 1]` reach the element
//! through two different places, and nothing reasoning about places can see
//! that they are one element. This pass finds the temps that hold the same
//! value and rewrites every index projection through a later one to name the
//! earliest, so the second write's place is the first one's.
//!
//! A temp takes part when one statement writes it, by arithmetic on locals and
//! literals, and the body never takes its address or changes its storage; no
//! local its expression reads may have its address taken either, since a
//! store through that address would change it unseen. A forward must-analysis
//! carries the temps whose expression still has the value the temp holds: a
//! temp joins it at its write, and leaves it when any local its expression
//! reads is written or has its storage changed.
//!
//! A temp written while another holding the same expression is in the set is
//! named by that one wherever it indexes a place. The earlier temp's write
//! comes before the later's on every path and, with nothing its expression
//! reads changed in between, gave it the same value. Lowering writes an index
//! temp just before the places it indexes, so every use of the later temp
//! comes after its write; if the earlier write runs again on the way to such a
//! use, as in a loop, the later write runs again after it, by the same
//! argument, before the use is reached.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::literal::Literal;
use crate::mir::analysis::traversal::reverse_postorder;
use crate::mir::place::PlaceContext;
use crate::mir::visitor::{MutVisitor, Visitor};
use crate::mir::{
    BasicBlock, BinOp, Body, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
    UnOp,
};

/// One input of a numbered expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Term {
    Local(Local),
    Literal(Literal),
}

/// The value a temp is written, as far as its equality with another's goes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Expression {
    Binary(BinOp, Term, Term),
    Unary(UnOp, Term),
}

impl Expression {
    fn of(rvalue: &Rvalue) -> Option<Self> {
        match rvalue {
            Rvalue::BinaryOp(op, left, right) => {
                Some(Self::Binary(*op, Term::of(left)?, Term::of(right)?))
            }
            Rvalue::UnaryOp(op, operand) => Some(Self::Unary(*op, Term::of(operand)?)),
            Rvalue::Use(_)
            | Rvalue::Ref(_)
            | Rvalue::Cast(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Aggregate(..) => None,
        }
    }

    fn reads(&self) -> impl Iterator<Item = Local> + '_ {
        let terms: [Option<&Term>; 2] = match self {
            Self::Binary(_, left, right) => [Some(left), Some(right)],
            Self::Unary(_, operand) => [Some(operand), None],
        };
        terms.into_iter().flatten().filter_map(|term| match term {
            Term::Local(local) => Some(*local),
            Term::Literal(_) => None,
        })
    }
}

impl Term {
    fn of(operand: &Operand) -> Option<Self> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
                Some(Self::Local(place.local))
            }
            Operand::Constant(constant) => Some(Self::Literal(constant.literal.clone())),
            Operand::Copy(_) | Operand::Move(_) | Operand::Function(_) => None,
        }
    }
}

/// A temp the pass may rename to, or from.
struct Candidate {
    local: Local,
    expression: Expression,
}

/// Rewrites every index through a temp holding a value an earlier temp
/// already holds to index through the earlier one. Returns true if any index
/// was rewritten.
pub fn number_indices(body: &mut Body) -> bool {
    let candidates = candidates(body);
    if candidates.len() < 2 {
        return false;
    }
    let renames = Numbering::of(&candidates).renames(body);
    if renames.is_empty() {
        return false;
    }
    IndexRenamer(&renames).visit_body(body);
    true
}

/// Every temp written once by arithmetic on locals and literals, used as an
/// index, whose storage and address the body never touches, with the
/// expression it is written.
fn candidates(body: &Body) -> Vec<Candidate> {
    let mut writes = vec![0usize; body.local_decls.len()];
    for place in body.written_places() {
        if let Some(count) = writes.get_mut(place.local.0) {
            *count += 1;
        }
    }
    let addressed = addressed_locals(body);
    let with_storage = locals_with_storage(body);
    let indexes = index_locals(body);
    let is_unaddressed = |local: Local| !addressed.contains(&local);
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(place, rvalue) if place.projection.is_empty() => {
                Some((place.local, Expression::of(rvalue)?))
            }
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        })
        .filter(|(local, expression)| {
            writes.get(local.0) == Some(&1)
                && indexes.contains(local)
                && is_unaddressed(*local)
                && !with_storage.contains(local)
                && expression.reads().all(is_unaddressed)
        })
        .map(|(local, expression)| Candidate { local, expression })
        .collect()
}

/// Every local whose address the body takes: a store through the address may
/// change it where no write to it is seen.
fn addressed_locals(body: &Body) -> HashSet<Local> {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place))
            | StatementKind::Reassign(_, Rvalue::Ref(place)) => Some(place.local),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        })
        .collect()
}

/// Every local whose storage the body starts or ends somewhere.
fn locals_with_storage(body: &Body) -> HashSet<Local> {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(storage_changed)
        .collect()
}

/// The local whose storage `statement` starts or ends.
fn storage_changed(statement: &Statement) -> Option<Local> {
    match &statement.kind {
        StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => Some(place.local),
        StatementKind::Assign(..)
        | StatementKind::Reassign(..)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// Every local some place of the body is indexed by.
fn index_locals(body: &Body) -> HashSet<Local> {
    struct Indexes(HashSet<Local>);
    impl Visitor for Indexes {
        fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
            self.0.extend(place.index_locals());
        }
    }
    let mut indexes = Indexes(HashSet::new());
    indexes.visit_body(body);
    indexes.0
}

/// What each statement does to the set of temps whose expression still has
/// the value they hold, resolved against the candidates once.
struct Numbering<'c> {
    candidates: &'c [Candidate],
    /// By candidate local: its index in `candidates`.
    by_local: HashMap<Local, usize>,
    /// By local: the candidates whose expression reads it.
    readers: HashMap<Local, Vec<usize>>,
}

/// The candidates still holding their expression's value, by index.
type Available = BTreeSet<usize>;

impl<'c> Numbering<'c> {
    fn of(candidates: &'c [Candidate]) -> Self {
        let by_local = (candidates.iter().enumerate())
            .map(|(index, candidate)| (candidate.local, index))
            .collect();
        let mut readers: HashMap<Local, Vec<usize>> = HashMap::new();
        for (index, candidate) in candidates.iter().enumerate() {
            for local in candidate.expression.reads() {
                readers.entry(local).or_default().push(index);
            }
        }
        Self {
            candidates,
            by_local,
            readers,
        }
    }

    /// The earliest temp each later one is renamed to: a later temp named by
    /// one that is itself renamed is named by where that one goes.
    fn renames(&self, body: &Body) -> HashMap<Local, Local> {
        let direct = self.direct_renames(body);
        (direct.iter())
            .map(|(later, earlier)| {
                let mut earliest = *earlier;
                // Each step names a temp written before the one it renames, so
                // the walk ends; the bound keeps it ending whatever the map.
                for _ in 0..direct.len() {
                    match direct.get(&earliest) {
                        Some(next) => earliest = *next,
                        None => break,
                    }
                }
                (*later, earliest)
            })
            .collect()
    }

    /// The earlier temp each later one is written the value of.
    fn direct_renames(&self, body: &Body) -> HashMap<Local, Local> {
        let entry = self.solve(body);
        let mut renames = HashMap::new();
        for (block, state) in entry.iter().enumerate() {
            let (Some(mut available), Some(data)) = (state.clone(), body.basic_blocks.get(block))
            else {
                continue;
            };
            for statement in &data.statements {
                if let Some((later, earlier)) = self.rename_at(statement, &available) {
                    renames.insert(later, earlier);
                }
                self.apply_statement(statement, &mut available);
            }
        }
        renames
    }

    /// The temp `statement` writes and the earlier one already holding its
    /// value, when one is.
    fn rename_at(&self, statement: &Statement, available: &Available) -> Option<(Local, Local)> {
        let StatementKind::Assign(place, _) = &statement.kind else {
            return None;
        };
        let later = *self.by_local.get(&place.local)?;
        let expression = &self.candidates.get(later)?.expression;
        available
            .iter()
            .filter_map(|index| self.candidates.get(*index))
            .find(|earlier| earlier.local != place.local && earlier.expression == *expression)
            .map(|earlier| (place.local, earlier.local))
    }

    /// The fixpoint of the entry states, in reverse postorder.
    fn solve(&self, body: &Body) -> Vec<Option<Available>> {
        let order = reverse_postorder(body);
        let mut entry: Vec<Option<Available>> = vec![None; body.basic_blocks.len()];
        if let Some(first) = entry.first_mut() {
            *first = Some(Available::new());
        }
        let mut changed = true;
        while changed {
            changed = false;
            for &block in &order {
                let Some(mut state) = entry.get(block).cloned().flatten() else {
                    continue;
                };
                self.apply_block(body, block, &mut state);
                let successors = (body.basic_blocks.get(block))
                    .and_then(|data| data.terminator.as_ref())
                    .map(|terminator| terminator.successors())
                    .unwrap_or_default();
                for successor in successors {
                    if let Some(slot) = entry.get_mut(successor.0) {
                        changed |= meet_into(slot, &state);
                    }
                }
            }
        }
        entry
    }

    fn apply_block(&self, body: &Body, block: usize, available: &mut Available) {
        let Some(data) = body.basic_blocks.get(block) else {
            return;
        };
        for statement in &data.statements {
            self.apply_statement(statement, available);
        }
        for place in data.terminator.iter().flat_map(|t| t.written_places()) {
            self.forget_readers_of(place.local, available);
        }
    }

    fn apply_statement(&self, statement: &Statement, available: &mut Available) {
        if let Some(local) = storage_changed(statement) {
            self.forget_readers_of(local, available);
        }
        let Some(place) = statement.written_place() else {
            return;
        };
        self.forget_readers_of(place.local, available);
        if let Some(&index) = self.by_local.get(&place.local) {
            available.insert(index);
        }
    }

    fn forget_readers_of(&self, local: Local, available: &mut Available) {
        for index in self.readers.get(&local).into_iter().flatten() {
            available.remove(index);
        }
    }
}

/// Intersects `state` into the entry state of a block, treating an unset one
/// as holding every temp. Returns true if the entry state changed.
fn meet_into(slot: &mut Option<Available>, state: &Available) -> bool {
    match slot {
        None => {
            *slot = Some(state.clone());
            true
        }
        Some(known) => {
            let before = known.len();
            known.retain(|index| state.contains(index));
            known.len() != before
        }
    }
}

/// Renames the index local of every place.
struct IndexRenamer<'r>(&'r HashMap<Local, Local>);

impl MutVisitor for IndexRenamer<'_> {
    fn visit_place(&mut self, place: &mut Place, _context: PlaceContext, _location: BasicBlock) {
        for elem in &mut place.projection {
            if let PlaceElem::Index(local) = elem {
                if let Some(earlier) = self.0.get(local) {
                    *local = *earlier;
                }
            }
        }
    }
}
