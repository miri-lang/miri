// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Drops a uniqueness check an earlier one already answered.
//!
//! A write through a path first asks, for each value on it and outermost
//! first, whether anything else holds that value, and copies the value when
//! something does (`lowering::expression::unshare`). Once a place has been
//! found unshared, or has just been given a value only it holds, nothing makes
//! it shared again until the program retains, copies, moves, hands to a call or
//! replaces what it holds — so a later check on the same place before any of
//! those answers no, and is replaced by a jump to the path that answer takes.
//!
//! A forward must-analysis carries the set of places known to hold a value
//! nothing else holds. A place enters it on the edge a check takes when the
//! answer is no, and when it is assigned a struct just built, the result of a
//! runtime call that allocates ([`returns_unshared`]), or a value moved out of
//! a whole local known unshared that nothing else reads — no retain, release,
//! copy or second move of that local anywhere in the body — so no alias of the
//! value outlives the move. A place enters only while every place it is
//! reached through is in the set, so a path through a class instance, which
//! is shared by design and never checked, never yields a fact; neither does a
//! place rooted at or indexed by a parameter, a captured binding, a
//! gpu-resident one or a local whose address is taken.
//!
//! Two index locals may name one element, so a read or write that reaches an
//! element through any index is taken to reach every element of that
//! collection, and assigning an index local forgets every place indexed by it.
//! Storing a fresh value into one element is the exception: the collection's
//! other elements known unshared stay known, since each is still held by the
//! collection alone, while everything inside the element replaced is
//! forgotten.
//!
//! Blocks only a removed check's copy path reached become unreachable; they
//! are emptied and end in `unreachable`, and no block is renumbered. A
//! removed check is also one place fewer where a run under the heap guard
//! validates the block written through: the guard still sees that block at
//! the check the analysis kept before it.

mod dataflow;
mod effects;
mod facts;
mod places;

use std::cell::OnceCell;
use std::collections::HashSet;

use crate::mir::optimization::OptimizationPass;
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    AggregateKind, BasicBlock, Body, Discriminant, Local, Operand, Place, Rvalue, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::{returns_unshared, rt};
use dataflow::Dataflow;
use effects::Program;
use places::PlaceTable;

/// The pass; see the module documentation.
pub struct UnshareElision;

impl OptimizationPass for UnshareElision {
    fn run(&mut self, body: &mut Body) -> bool {
        elide_answered_checks(body)
    }

    fn name(&self) -> &'static str {
        "Unshare Check Elision"
    }
}

/// A uniqueness check as lowering emits it: `call` asks whether `place` is
/// shared and continues at `switch`, which branches on the answer, taking
/// `unshared` when it is no.
struct Check {
    call: usize,
    switch: usize,
    place: Place,
    unshared: BasicBlock,
}

/// Replaces every check whose place is known unshared where it is asked with a
/// jump to the path its answer takes. Returns true if any check was removed.
fn elide_answered_checks(body: &mut Body) -> bool {
    if !asks_whether_shared(body) {
        return false;
    }
    let answered = answered_checks(body);
    if answered.is_empty() {
        return false;
    }
    let unreachable_before: HashSet<usize> = body.find_unreachable_blocks().into_iter().collect();
    for (call, target) in answered {
        replace_terminator(body, call, TerminatorKind::Goto { target });
    }
    clear_newly_unreachable(body, &unreachable_before);
    true
}

/// Whether any block of `body` ends by asking whether a value is shared.
fn asks_whether_shared(body: &Body) -> bool {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .any(|terminator| match &terminator.kind {
            TerminatorKind::Call { func, .. } => {
                func.called_runtime_name() == Some(rt::RC_IS_SHARED)
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => false,
        })
}

/// Each check whose place is known unshared where it is asked, as the block
/// asking and the block its answer "no" continues at.
fn answered_checks(body: &Body) -> Vec<(usize, BasicBlock)> {
    let uses = LocalUseCounts::of(body);
    let checks = recognize_checks(body, &uses);
    let table = PlaceTable::of(body, &checks);
    if checks.iter().all(|check| table.id(&check.place).is_none()) {
        return Vec::new();
    }
    let mut asks = vec![false; body.basic_blocks.len()];
    for check in &checks {
        if let Some(slot) = asks.get_mut(check.call) {
            *slot = true;
        }
    }
    let program = Program::of(body, &table, &uses, &asks);
    let dataflow = Dataflow::new(body, &program, &table, &checks);
    let entry = dataflow.solve(body);
    checks
        .iter()
        .filter(|check| dataflow.is_answered(check, &entry))
        .map(|check| (check.call, check.unshared))
        .collect()
}

/// Ends `block` with `kind`, keeping the span of the terminator it replaces.
fn replace_terminator(body: &mut Body, block: usize, kind: TerminatorKind) {
    let fallback = body.span;
    let Some(data) = body.basic_blocks.get_mut(block) else {
        return;
    };
    let span = data.terminator.as_ref().map_or(fallback, |old| old.span);
    data.terminator = Some(Terminator::new(kind, span));
}

/// Empties every block the rewrite cut off, ending it in `unreachable`.
fn clear_newly_unreachable(body: &mut Body, unreachable_before: &HashSet<usize>) {
    for index in body.find_unreachable_blocks() {
        if unreachable_before.contains(&index) {
            continue;
        }
        if let Some(data) = body.basic_blocks.get_mut(index) {
            data.statements.clear();
        }
        replace_terminator(body, index, TerminatorKind::Unreachable);
    }
}

/// Every check in `body` of exactly the shape lowering emits.
fn recognize_checks(body: &Body, uses: &LocalUseCounts) -> Vec<Check> {
    let predecessors = body.predecessor_counts();
    (0..body.basic_blocks.len())
        .filter_map(|call| recognize_check(body, call, &predecessors, uses))
        .collect()
}

/// The check `call` ends with, when it asks about one place, writes the answer
/// into a local nothing else writes, and continues at a block reached from it
/// alone that does nothing but branch on that answer.
fn recognize_check(
    body: &Body,
    call: usize,
    predecessors: &[usize],
    uses: &LocalUseCounts,
) -> Option<Check> {
    let TerminatorKind::Call {
        func,
        args,
        destination,
        target: Some(switch),
        arg_modes: _,
        arg_handles: _,
    } = &body.basic_blocks.get(call)?.terminator.as_ref()?.kind
    else {
        return None;
    };
    if func.called_runtime_name() != Some(rt::RC_IS_SHARED) || !destination.projection.is_empty() {
        return None;
    }
    let [Operand::Copy(place)] = args.as_slice() else {
        return None;
    };
    let switch_block = body.basic_blocks.get(switch.0)?;
    let is_entered_from_call_alone = predecessors.get(switch.0) == Some(&1);
    if switch.0 == call || !is_entered_from_call_alone || !switch_block.statements.is_empty() {
        return None;
    }
    let unshared = branch_on_answer(switch_block.terminator.as_ref()?, destination.local)?;
    uses.is_defined_once(destination.local).then(|| Check {
        call,
        switch: switch.0,
        place: place.clone(),
        unshared,
    })
}

/// The block `terminator` takes when `answer` is 0, when it branches on
/// `answer` alone to one block for 0 and a different one otherwise.
fn branch_on_answer(terminator: &Terminator, answer: Local) -> Option<BasicBlock> {
    let TerminatorKind::SwitchInt {
        discr: Operand::Copy(discr),
        targets,
        otherwise,
    } = &terminator.kind
    else {
        return None;
    };
    let [(value, unshared)] = targets.as_slice() else {
        return None;
    };
    let is_answer = discr.local == answer && discr.projection.is_empty();
    (is_answer && *value == Discriminant::from(0) && unshared != otherwise).then_some(*unshared)
}

/// Where a value only the place it is stored into holds may come from, as far
/// as the rvalue alone tells.
enum FreshSource<'r> {
    /// A struct just built.
    Built,
    /// A value moved out of a whole local.
    Moved(&'r Place),
    Neither,
}

fn fresh_source(rvalue: &Rvalue) -> FreshSource<'_> {
    match rvalue {
        Rvalue::Aggregate(kind, _) if is_struct(kind) => FreshSource::Built,
        Rvalue::Use(Operand::Move(source)) if source.projection.is_empty() => {
            FreshSource::Moved(source)
        }
        Rvalue::Use(_)
        | Rvalue::Aggregate(..)
        | Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => FreshSource::Neither,
    }
}

fn is_struct(kind: &AggregateKind) -> bool {
    match kind {
        AggregateKind::Struct(_) => true,
        AggregateKind::Tuple
        | AggregateKind::Array
        | AggregateKind::Class(_)
        | AggregateKind::List
        | AggregateKind::FormattedString
        | AggregateKind::Set
        | AggregateKind::Map
        | AggregateKind::Enum(..)
        | AggregateKind::Option
        | AggregateKind::Closure(..) => false,
    }
}

/// Whether `func` names a runtime function handing back a block it has just
/// allocated.
fn allocates(func: &Operand) -> bool {
    func.called_runtime_name().is_some_and(returns_unshared)
}

/// How many times each local is written, and — counted only when first asked
/// for — how many times it is mentioned in any other way.
struct LocalUseCounts<'b> {
    body: &'b Body,
    definitions: Vec<usize>,
    mentions: OnceCell<Vec<usize>>,
}

impl<'b> LocalUseCounts<'b> {
    fn of(body: &'b Body) -> Self {
        let mut definitions = vec![0; body.local_decls.len()];
        for place in body.written_places() {
            if let Some(count) = definitions.get_mut(place.local.0) {
                *count += 1;
            }
        }
        Self {
            body,
            definitions,
            mentions: OnceCell::new(),
        }
    }

    fn is_defined_once(&self, local: Local) -> bool {
        self.definitions.get(local.0) == Some(&1)
    }

    /// Whether the body reads `local` once, other than by writing it: a value
    /// moved out of it is then held by nothing else afterwards. A release or a
    /// retain counts as a read.
    fn is_read_once(&self, local: Local) -> bool {
        let mentions = self.mentions.get_or_init(|| {
            let mut mentions = Mentions(vec![0; self.body.local_decls.len()]);
            mentions.visit_body(self.body);
            mentions.0
        });
        let mentioned = mentions.get(local.0).copied().unwrap_or(0);
        let defined = self.definitions.get(local.0).copied().unwrap_or(0);
        mentioned.saturating_sub(defined) == 1
    }
}

/// How many times each local is used or written, as the root of a place or as
/// an index into one.
struct Mentions(Vec<usize>);

impl Visitor for Mentions {
    fn visit_local(&mut self, local: Local, context: PlaceContext, _location: BasicBlock) {
        let is_use = match context {
            PlaceContext::NonMutatingUse | PlaceContext::MutatingUse => true,
            PlaceContext::StorageLive | PlaceContext::StorageDead => false,
        };
        if let Some(count) = self.0.get_mut(local.0).filter(|_| is_use) {
            *count += 1;
        }
    }
}
