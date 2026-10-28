// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Gives a loop compared with a bound other than a length a second copy,
//! entered only when that bound cannot pass the length of any list the loop
//! indexes, so the in-bounds pass can prove the copy's accesses.
//!
//! `while i < n: … l[i] …` proves nothing when `n` is a parameter: nothing
//! ties `n` to `l.length()`. A guard in front of the loop can:
//!
//! ```text
//! L = Len(l); c = Le(n, L); switchInt(c) -> [1: copy of the loop, otherwise: loop]
//! ```
//!
//! Each guard asks `bound + d <= L` of one list for a margin `d`: `Le(bound,
//! L)` for 0, `Lt(bound, L)` for 1, and `m = Sub(L, d); Le(bound, m)` above
//! that. On the copy's side it holds and `L` stays `l`'s length, so the
//! in-bounds pass finds the copy's accesses inside. When the guard fails,
//! the loop runs as it always did, checking every access, so a loop that
//! runs past the end stops at the same iteration with the same report. A
//! guard only measures and compares: a length is at least 0 and `d` at most
//! [`MAX_OFFSET`] + 1, so `L - d` cannot overflow, and the guard never adds
//! to or multiplies the bound.
//!
//! Two shapes of loop get a copy. One counts an index `i`, never negative,
//! up to a bound `n` the loop never writes: its header switches on `i < n`
//! or `i <= n`, and it indexes a list by `i` or by `x = i + c` for a
//! constant `0 <= c <= MAX_OFFSET` computed in the loop. The guard's margin
//! for a list is the largest such `c`, plus 1 for `i <= n`: `i < n` and
//! `n + c <= L` give `i + c < L`, and `i <= n` needs `n + c < L`. The other
//! is a two-pointer loop: its header switches on `lo < hi` or `lo <= hi`,
//! `lo` never negative and only stepped up by a constant, `hi` only stepped
//! down by a constant (directly or through a copy) and at least once, and
//! it indexes a list by `lo` or `hi`. Its guard is `hi < L`: stepping down
//! keeps `hi` below `L`, `lo <= hi` keeps `hi` at least 0, and `lo <= hi`
//! keeps `lo` below `L` too.
//!
//! In both, the bound is an `int` whose address is never taken and that no
//! closure captures; no list is guarded that the bound simply measures; and
//! the loop is innermost, indexes a list whose header it or a loop around it
//! leaves unchanged, calls nothing but runtime entry points that leave every
//! header as it was, holds no `Phi`, nor do the blocks it exits to, and is
//! small: at most [`MAX_BLOCKS`] blocks, [`MAX_STATEMENTS`] statements and
//! [`MAX_GUARDS`] lists, one guard each.
//!
//! The copy shares every local with the loop and keeps every statement,
//! storage marker and reference-count operation as it was: each run of the
//! loop takes exactly one of the two. Loops are chosen on the body as it
//! stands and copied in header order, the copies' blocks and the guards'
//! locals appended, so two compilations number them alike; the edges into
//! each header are kept up to date as blocks are appended, so a copy that
//! leaves straight into a later loop enters that loop's guards.
//!
//! The pass runs before the stable-collections pass, which then records each
//! copy as a loop of its own, and so before the in-bounds pass; a body that
//! already carries either record is left alone, since copying would move the
//! blocks they name. A body that runs on a GPU is left alone.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::analysis::header_reach::runtime_call_leaves_headers;
use crate::mir::analysis::loops::{
    comparison_before_switch, natural_loops, predecessors, writes_whole, Comparison, NaturalLoop,
};
use crate::mir::analysis::non_negative::{constant_step, non_negative_locals, Step};
use crate::mir::analysis::stable_headers::{is_single_entry, StableHeaders};
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Constant, Discriminant, Local, LocalDecl, Operand,
    Place, PlaceElem, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};

/// The most blocks a loop may have to be copied.
pub const MAX_BLOCKS: usize = 16;
/// The most statements a loop may hold, over all its blocks, to be copied.
pub const MAX_STATEMENTS: usize = 64;
/// The most lists a copied loop may index, each checked by a guard of its own.
pub const MAX_GUARDS: usize = 4;
/// The largest constant `c` an index `i + c` may add for its accesses to
/// count towards a guard; one further out stays checked.
pub const MAX_OFFSET: u64 = 64;

/// A loop chosen for a copy, with what its guards compare.
struct Chosen {
    found: NaturalLoop,
    /// The local each guard keeps below a length: the bound `n` the index
    /// counts up to, or the high pointer `hi` counting down towards it.
    bound: Local,
    /// The lists the loop indexes, in local order, one guard each, with the
    /// margin `d` the guard asks of the bound: `bound + d <= Len(list)`.
    collections: Vec<(Local, u64)>,
}

/// Copies every loop of `body` a guard on its bound lets the in-bounds pass
/// prove, entering the copy through that guard. Returns true if any loop
/// was copied.
pub fn version_bounded_loops(body: &mut Body) -> bool {
    if body.is_gpu() || !body.stable_collections.is_empty() || !body.in_bounds.is_empty() {
        return false;
    }
    let chosen = chosen_loops(body);
    let mut predecessors = predecessors(body);
    for each in &chosen {
        version(body, each, &mut predecessors);
    }
    !chosen.is_empty()
}

/// Every loop of `body` worth a copy, in header order.
fn chosen_loops(body: &Body) -> Vec<Chosen> {
    let loops = natural_loops(body, &DominatorTree::compute(body));
    if loops.is_empty() {
        return Vec::new();
    }
    let Some(mut headers) = StableHeaders::of(body) else {
        return Vec::new();
    };
    let facts = BodyFacts::of(body);
    (loops.iter())
        .filter(|found| is_innermost(found, &loops))
        .filter_map(|found| facts.choose(found, &loops, &mut headers))
        .collect()
}

/// What is decided once for the body the loops are chosen in.
struct BodyFacts<'a> {
    body: &'a Body,
    non_negative: HashSet<usize>,
    address_taken: HashSet<Local>,
    predecessors: Vec<Vec<usize>>,
}

impl<'a> BodyFacts<'a> {
    fn of(body: &'a Body) -> Self {
        Self {
            body,
            non_negative: non_negative_locals(body),
            address_taken: address_taken(body),
            predecessors: predecessors(body),
        }
    }

    /// `found` with its guards, if it is worth a copy: its header compares
    /// a never-negative index `i` with a bound the loop never writes, or a
    /// low pointer counting up with a high pointer counting down.
    fn choose(
        &self,
        found: &NaturalLoop,
        loops: &[NaturalLoop],
        headers: &mut StableHeaders,
    ) -> Option<Chosen> {
        if !self.is_copyable(found) {
            return None;
        }
        let Comparison { lhs, rhs, strict } = self.header_comparison(found)?;
        if !self.non_negative.contains(&lhs.0) || lhs == rhs || !self.is_steady_int(rhs) {
            return None;
        }
        let margins = if !writes_in_loop(self.body, found, rhs) {
            margins_below_a_bound(self.body, found, lhs, strict)
        } else if steps_only(self.body, found, rhs, StepWay::Down)
            && steps_only(self.body, found, lhs, StepWay::Up)
        {
            margins_of_two_pointers(self.body, found, lhs, rhs)
        } else {
            return None;
        };
        let collections: Vec<(Local, u64)> = (margins.into_iter())
            .filter(|(collection, _)| !is_length_of(self.body, rhs, *collection))
            .filter(|(collection, _)| is_stable_around(headers, loops, found, *collection))
            .collect();
        (!collections.is_empty() && collections.len() <= MAX_GUARDS).then(|| Chosen {
            found: found.clone(),
            bound: rhs,
            collections,
        })
    }

    /// Whether `found` may be copied: small, entered only through its
    /// header, free of `Phi`, and calling nothing that may change a header.
    fn is_copyable(&self, found: &NaturalLoop) -> bool {
        let statements: usize = (found.blocks.iter())
            .filter_map(|block| self.body.basic_blocks.get(*block))
            .map(|data| data.statements.len())
            .sum();
        found.header != 0
            && found.blocks.len() <= MAX_BLOCKS
            && statements <= MAX_STATEMENTS
            && is_single_entry(found, &self.predecessors)
            && !holds_phi(self.body, found)
            && calls_keep_headers(self.body, found)
    }

    /// The comparison the header of `found` switches on, when its true edge
    /// stays in the loop.
    fn header_comparison(&self, found: &NaturalLoop) -> Option<Comparison> {
        let terminator = self
            .body
            .basic_blocks
            .get(found.header)?
            .terminator
            .as_ref()?;
        let TerminatorKind::SwitchInt { discr, targets, .. } = &terminator.kind else {
            return None;
        };
        let [(value, target)] = targets.as_slice() else {
            return None;
        };
        if *value != Discriminant::bool_true() || !found.blocks.contains(&target.0) {
            return None;
        }
        comparison_before_switch(self.body, found.header, discr.bare_local()?)
    }

    /// Whether `bound` is an `int` that only the statements of this body
    /// give a value: its address is never taken and no closure captures it.
    fn is_steady_int(&self, bound: Local) -> bool {
        let is_int = (self.body.local_decls.get(bound.0))
            .is_some_and(|decl| decl.ty.kind == TypeKind::Int && decl.device_handle.is_none());
        is_int
            && !self.address_taken.contains(&bound)
            && !self.body.env_capture_locals.contains(&bound)
    }
}

/// Which way a pointer of a two-pointer loop moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepWay {
    Up,
    Down,
}

/// Whether every write of `local` in `found` steps it `way` by a constant
/// at least 0: `local = local ± c`, directly, or as a whole copy of a local
/// each of whose writes in `found` is that step and the last of which comes
/// earlier in the copy's own block — so the copy takes this iteration's
/// step, not one left over from before. A loop stepping down must step at
/// least once; no terminator of it may write `local`.
fn steps_only(body: &Body, found: &NaturalLoop, local: Local, way: StepWay) -> bool {
    let is_step = |rvalue: &Rvalue| is_step_of(rvalue, local, way);
    let is_stepped = |statements: &[Statement], position: usize| {
        let Some(rvalue) = statements.get(position).and_then(written_value) else {
            return false;
        };
        is_step(rvalue)
            || copied_local(rvalue).is_some_and(|source| {
                let earlier = statements.get(..position).unwrap_or_default();
                let made_before = (earlier.iter().rev())
                    .find(|statement| writes_whole(statement, source))
                    .and_then(written_value)
                    .is_some_and(is_step);
                source != local
                    && made_before
                    && loop_writes(body, found, source)
                        .iter()
                        .all(|write| write.is_some_and(is_step))
            })
    };
    let mut writes = 0;
    for data in (found.blocks.iter()).filter_map(|block| body.basic_blocks.get(*block)) {
        for (position, statement) in data.statements.iter().enumerate() {
            if writes_whole(statement, local) {
                if !is_stepped(&data.statements, position) {
                    return false;
                }
                writes += 1;
            }
        }
    }
    (way == StepWay::Up || writes > 0) && !terminator_writes(body, found, local)
}

/// The value `statement` gives the place it writes; `None` for one that
/// starts or ends storage or writes nothing.
fn written_value(statement: &Statement) -> Option<&Rvalue> {
    match &statement.kind {
        StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) => Some(rvalue),
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// Whether `rvalue` is `local` stepped `way` by a constant at least 0.
fn is_step_of(rvalue: &Rvalue, local: Local, way: StepWay) -> bool {
    let Rvalue::BinaryOp(op, lhs, rhs) = rvalue else {
        return false;
    };
    match constant_step(*op, lhs, rhs) {
        Some((source, Step::Up(_))) => source == local && way == StepWay::Up,
        Some((source, Step::Down(_))) => source == local && way == StepWay::Down,
        None => false,
    }
}

/// The value every statement of `found` writing the whole of `local` gives
/// it, `None` for one that starts or ends its storage.
fn loop_writes<'a>(body: &'a Body, found: &NaturalLoop, local: Local) -> Vec<Option<&'a Rvalue>> {
    (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .flat_map(|data| &data.statements)
        .filter(|statement| writes_whole(statement, local))
        .map(written_value)
        .collect()
}

/// The whole local `rvalue` copies, if it is a whole-local copy.
fn copied_local(rvalue: &Rvalue) -> Option<Local> {
    match rvalue {
        Rvalue::Use(operand) => operand.bare_local(),
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::Aggregate(..)
        | Rvalue::Phi(_)
        | Rvalue::AtomicOp { .. } => None,
    }
}

/// Whether a terminator of `found` writes `local`.
fn terminator_writes(body: &Body, found: &NaturalLoop, local: Local) -> bool {
    (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .filter_map(|data| data.terminator.as_ref())
        .flat_map(Terminator::written_places)
        .any(|place| place.local == local)
}

/// The margin a guard asks of the bound `n` of `i < n` (or `i <= n` when
/// not `strict`) for each list `found` indexes by `i` or by `i + c`: `c`,
/// the largest such offset up to [`MAX_OFFSET`], plus 1 for `i <= n`.
/// `i < n` and `n + c <= L` give `i + c < L`; `i <= n` needs `n + c < L`.
fn margins_below_a_bound(
    body: &Body,
    found: &NaturalLoop,
    index: Local,
    strict: bool,
) -> Vec<(Local, u64)> {
    let mut offsets = offsets_from(body, found, index);
    offsets.insert(index, 0);
    let widen = u64::from(!strict);
    indexed_by(body, found, &offsets)
        .into_iter()
        .map(|(collection, offset)| (collection, offset + widen))
        .collect()
}

/// The margin of 1 a guard `hi < L` gives each list `found` indexes by
/// either pointer of `lo < hi` or `lo <= hi`: `hi` only steps down from
/// below `L`, and `lo`, never negative and at most `hi`, keeps `hi` at
/// least 0 and itself below `L`.
fn margins_of_two_pointers(
    body: &Body,
    found: &NaturalLoop,
    low: Local,
    high: Local,
) -> Vec<(Local, u64)> {
    let pointers = HashMap::from([(low, 0), (high, 0)]);
    (indexed_by(body, found, &pointers).into_iter())
        .map(|(collection, _)| (collection, 1))
        .collect()
}

/// Each local `found` gives the value `index + c`, with the largest such
/// constant `c` up to [`MAX_OFFSET`].
fn offsets_from(body: &Body, found: &NaturalLoop, index: Local) -> HashMap<Local, u64> {
    let mut offsets: HashMap<Local, u64> = HashMap::new();
    let statements = (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .flat_map(|data| &data.statements);
    for statement in statements {
        let (StatementKind::Assign(place, Rvalue::BinaryOp(op, lhs, rhs))
        | StatementKind::Reassign(place, Rvalue::BinaryOp(op, lhs, rhs))) = &statement.kind
        else {
            continue;
        };
        let Some((source, Step::Up(offset))) = constant_step(*op, lhs, rhs) else {
            continue;
        };
        if source == index && place.projection.is_empty() && offset <= MAX_OFFSET {
            let largest = offsets.entry(place.local).or_default();
            *largest = (*largest).max(offset);
        }
    }
    offsets
}

/// Whether no other loop of `loops` has its header inside `found`.
fn is_innermost(found: &NaturalLoop, loops: &[NaturalLoop]) -> bool {
    !loops
        .iter()
        .any(|other| other.header != found.header && found.blocks.contains(&other.header))
}

/// Whether a block of `found`, or a block it exits to, holds a `Phi`: one
/// names the blocks it is entered from, which a copy would add to.
fn holds_phi(body: &Body, found: &NaturalLoop) -> bool {
    let exits = (found.blocks.iter())
        .flat_map(|block| successors_of(body, *block))
        .filter(|successor| !found.blocks.contains(successor));
    let blocks: BTreeSet<usize> = found.blocks.iter().copied().chain(exits).collect();
    (blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .flat_map(|data| &data.statements)
        .any(|statement| {
            matches!(
                &statement.kind,
                StatementKind::Assign(_, Rvalue::Phi(_))
                    | StatementKind::Reassign(_, Rvalue::Phi(_))
            )
        })
}

fn successors_of(body: &Body, block: usize) -> Vec<usize> {
    (body.basic_blocks.get(block))
        .and_then(|data| data.terminator.as_ref())
        .map(|terminator| terminator.successors().iter().map(|b| b.0).collect())
        .unwrap_or_default()
}

/// Whether every block of `found` ends in a jump, a switch, a trap, or a
/// call of a runtime entry point that leaves every header as it was — the
/// only calls across which the in-bounds pass keeps a length.
fn calls_keep_headers(body: &Body, found: &NaturalLoop) -> bool {
    (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .all(|data| {
            let Some(terminator) = &data.terminator else {
                return false;
            };
            match &terminator.kind {
                TerminatorKind::Goto { .. }
                | TerminatorKind::SwitchInt { .. }
                | TerminatorKind::Unreachable => true,
                TerminatorKind::Call { func, args, .. } => func
                    .called_runtime_name()
                    .is_some_and(|name| runtime_call_leaves_headers(body, name, args)),
                TerminatorKind::VirtualCall { .. }
                | TerminatorKind::GpuLaunch { .. }
                | TerminatorKind::Return => false,
            }
        })
}

/// Whether a block of `found` gives `local` a value or bounds its storage.
fn writes_in_loop(body: &Body, found: &NaturalLoop, local: Local) -> bool {
    (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .any(|data| {
            data.statements.iter().any(|s| writes_whole(s, local))
                || (data.terminator.iter())
                    .flat_map(Terminator::written_places)
                    .any(|place| place.local == local)
        })
}

/// The locals whose address `body` takes anywhere.
fn address_taken(body: &Body) -> HashSet<Local> {
    (body.basic_blocks.iter())
        .flat_map(|data| &data.statements)
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

/// Whether every definition of `bound` in `body` measures `collection`, so
/// the in-bounds pass proves the loop without a copy.
fn is_length_of(body: &Body, bound: Local, collection: Local) -> bool {
    let mut definitions = (body.basic_blocks.iter())
        .flat_map(|data| &data.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue)
                if place.local == bound && place.projection.is_empty() =>
            {
                Some(rvalue)
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
        .peekable();
    definitions.peek().is_some()
        && definitions.all(|rvalue| {
            matches!(rvalue, Rvalue::Len(measured)
                if measured.local == collection && measured.projection.is_empty())
        })
}

/// Whether `found`, or a loop around it, leaves the header of `collection`
/// unchanged, so the stable-collections pass records it over the copy.
fn is_stable_around(
    headers: &mut StableHeaders,
    loops: &[NaturalLoop],
    found: &NaturalLoop,
    collection: Local,
) -> bool {
    (loops.iter())
        .filter(|outer| outer.blocks.is_superset(&found.blocks))
        .any(|outer| headers.leaves_every_header(outer) && headers.keeps_header(outer, collection))
}

/// The arrays and lists a block of `found` indexes at the first projection
/// by a local of `offsets`, in local order, each with the largest offset
/// of a local it is indexed by.
fn indexed_by(
    body: &Body,
    found: &NaturalLoop,
    offsets: &HashMap<Local, u64>,
) -> Vec<(Local, u64)> {
    let mut roots = IndexedBy {
        offsets,
        collections: BTreeMap::new(),
    };
    for block in &found.blocks {
        if let Some(data) = body.basic_blocks.get(*block) {
            roots.visit_basic_block(BasicBlock(*block), data);
        }
    }
    (roots.collections.into_iter())
        .filter(|(local, _)| {
            body.local_decls.get(*local).is_some_and(|decl| {
                matches!(
                    decl.ty.kind.as_builtin_collection(),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                )
            })
        })
        .map(|(local, offset)| (Local(local), offset))
        .collect()
}

/// Collects the root of every place indexed at its first projection by a
/// local of `offsets`, with the largest offset it is indexed at.
struct IndexedBy<'a> {
    offsets: &'a HashMap<Local, u64>,
    /// The largest offset of each collection, by local index.
    collections: BTreeMap<usize, u64>,
}

impl Visitor for IndexedBy<'_> {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        let Some(PlaceElem::Index(index)) = place.projection.first() else {
            return;
        };
        if let Some(offset) = self.offsets.get(index) {
            let largest = self.collections.entry(place.local.0).or_default();
            *largest = (*largest).max(*offset);
        }
    }
}

/// Appends a copy of `chosen`'s loop and its guards to `body`, and sends
/// every entry into the loop through the guards. `predecessors` holds the
/// edges of `body` and is kept up to date with the blocks appended and the
/// entries led to the guards, so a later loop finds a copy leaving into it.
fn version(body: &mut Body, chosen: &Chosen, predecessors: &mut Vec<Vec<usize>>) {
    let entries = chosen.found.entries(predecessors);
    let first_new = body.basic_blocks.len();
    let Some(copy_header) = copy_loop(body, &chosen.found) else {
        return;
    };
    let first_guard = append_guards(body, chosen, copy_header);
    record_edges_from(body, first_new, predecessors);
    let header = chosen.found.header;
    for entry in entries {
        let terminator =
            (body.basic_blocks.get_mut(entry)).and_then(|data| data.terminator.as_mut());
        if let Some(terminator) = terminator {
            terminator.replace_successor(BasicBlock(header), BasicBlock(first_guard));
        }
        if let Some(into_header) = predecessors.get_mut(header) {
            into_header.retain(|pred| *pred != entry);
        }
        if let Some(into_guard) = predecessors.get_mut(first_guard) {
            into_guard.push(entry);
        }
    }
}

/// Adds to `predecessors` the edges out of every block of `body` from
/// `first` on, the blocks just appended.
fn record_edges_from(body: &Body, first: usize, predecessors: &mut Vec<Vec<usize>>) {
    predecessors.resize(body.basic_blocks.len(), Vec::new());
    for block in first..body.basic_blocks.len() {
        for successor in successors_of(body, block) {
            if let Some(list) = predecessors.get_mut(successor) {
                list.push(block);
            }
        }
    }
}

/// Appends a copy of every block of `found`, its edges inside the loop led
/// to the copies and its exits left as they were. Returns the copy of the
/// header.
fn copy_loop(body: &mut Body, found: &NaturalLoop) -> Option<usize> {
    let start = body.basic_blocks.len();
    let copies: BTreeMap<usize, usize> = (found.blocks.iter())
        .enumerate()
        .map(|(offset, block)| (*block, start + offset))
        .collect();
    let blocks: Vec<BasicBlockData> = (found.blocks.iter())
        .map(|block| body.basic_blocks.get(*block).cloned())
        .collect::<Option<_>>()?;
    for mut data in blocks {
        if let Some(terminator) = data.terminator.as_mut() {
            for (original, copy) in &copies {
                terminator.replace_successor(BasicBlock(*original), BasicBlock(*copy));
            }
        }
        body.basic_blocks.push(data);
    }
    copies.get(&found.header).copied()
}

/// Appends one guard per list of `chosen`, each passing to the next and the
/// last to `copy_header`, each failing to the loop as it was. Returns the
/// first guard.
fn append_guards(body: &mut Body, chosen: &Chosen, copy_header: usize) -> usize {
    let first = body.basic_blocks.len();
    for (offset, (collection, margin)) in chosen.collections.iter().enumerate() {
        let passed = if offset + 1 == chosen.collections.len() {
            copy_header
        } else {
            first + offset + 1
        };
        let guard = guard_block(body, chosen, *collection, *margin, passed);
        body.basic_blocks.push(guard);
    }
    first
}

/// `L = Len(collection); c = Le(bound, L)` for a margin of 0, `Lt(bound, L)`
/// for 1, and `m = Sub(L, margin); c = Le(bound, m)` above that, then
/// `switchInt(c) -> [1: passed, otherwise: the loop]`. A length is at least
/// 0 and a margin at most [`MAX_OFFSET`] + 1, so `L - margin` never
/// overflows and the guard cannot trap. Declares the locals it computes.
fn guard_block(
    body: &mut Body,
    chosen: &Chosen,
    collection: Local,
    margin: u64,
    passed: usize,
) -> BasicBlockData {
    let span = body.span;
    let int = || LocalDecl::new(Type::new(TypeKind::Int, span), span);
    let length = body.new_local(int());
    let mut kinds = vec![StatementKind::Assign(
        Place::new(length),
        Rvalue::Len(Place::new(collection)),
    )];
    let (op, limit) = match margin {
        0 => (BinOp::Le, length),
        1 => (BinOp::Lt, length),
        2.. => {
            let limit = body.new_local(int());
            let less = Rvalue::BinaryOp(
                BinOp::Sub,
                Box::new(Operand::Copy(Place::new(length))),
                Box::new(int_constant(margin, span)),
            );
            kinds.push(StatementKind::Assign(Place::new(limit), less));
            (BinOp::Le, limit)
        }
    };
    let holds = body.new_local(LocalDecl::new(Type::new(TypeKind::Boolean, span), span));
    let compared = Rvalue::BinaryOp(
        op,
        Box::new(Operand::Copy(Place::new(chosen.bound))),
        Box::new(Operand::Copy(Place::new(limit))),
    );
    kinds.push(StatementKind::Assign(Place::new(holds), compared));
    let switch = TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(holds)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(passed))],
        otherwise: BasicBlock(chosen.found.header),
    };
    let mut guard = BasicBlockData::new(Some(Terminator::new(switch, span)));
    guard.statements = (kinds.into_iter())
        .map(|kind| Statement { kind, span })
        .collect();
    guard
}

/// The `int` constant `value`. [`MAX_OFFSET`] keeps a margin far inside an
/// `int`; were one past it, the guard would compare against `L - i64::MAX`,
/// stricter than asked, and the copy would only be taken less often.
fn int_constant(value: u64, span: Span) -> Operand {
    let value = i64::try_from(value).unwrap_or(i64::MAX);
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}
