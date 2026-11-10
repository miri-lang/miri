// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Drop elaboration: a binding a consuming site gives up is released exactly
//! once on every path, however many places end it.
//!
//! Lowering ends a binding's storage where its scope ends
//! ([`EndCause::Scope`]), and also where a consuming site gives its value up
//! ([`EndCause::Consumed`]): a call that keeps a resource, a `let` that moves
//! one into a new binding, a `value.drop()`. Each end is where reference
//! counting places the binding's release. A scope end reached after a
//! consuming end would release the value a second time, so this pass decides,
//! for every scope end of a binding with a consuming end, what the paths into
//! it hold:
//!
//! | Paths into the scope end | The end becomes |
//! |---|---|
//! | none consumed it | itself |
//! | every one consumed it | nothing |
//! | some consumed it | the end behind a test of the binding's drop flag |
//!
//! A drop flag is a `bool` local ([`FlagOf::Drop`]) set right after the
//! binding is bound whole and cleared right after its storage ends; it is
//! declared only for a binding some end is reached along paths that disagree.
//! A rebinding that would release the old value (`Reassign`) is treated the
//! same way: after a consumption on every path it releases nothing, and after
//! one on some paths it releases the old value behind the flag. A slot is never
//! cleared to mean "consumed".
//!
//! Only a consuming end makes a binding consumed: the pass reads the cause
//! written on each end, never the number of ends. A binding ended twice along
//! one path with no binding between — two scope ends, or a second end after a
//! consuming one — is a lowering defect, refused rather than repaired.
//!
//! The decision is the control-flow graph's, so it is made here, over the MIR
//! lowering leaves, before reference counting turns each surviving end into a
//! release.

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::block::{BasicBlock, BasicBlockData};
use crate::mir::terminator::{Discriminant, Terminator, TerminatorKind};
use crate::mir::{
    Body, Constant, EndCause, FlagOf, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind,
};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// What the paths into a point may have done to one binding since it was
/// last bound. No flag set means no path bound it yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Held {
    /// Some path bound it and has not ended it since.
    live: bool,
    /// Some path ended it at a consuming end and has not bound it since.
    consumed: bool,
    /// Some path ended it where its scope ends and has not bound it since.
    ended: bool,
}

impl Held {
    const LIVE: Held = Held {
        live: true,
        consumed: false,
        ended: false,
    };
    const CONSUMED: Held = Held {
        live: false,
        consumed: true,
        ended: false,
    };
    const ENDED: Held = Held {
        live: false,
        consumed: false,
        ended: true,
    };

    fn join(self, other: Held) -> Held {
        Held {
            live: self.live || other.live,
            consumed: self.consumed || other.consumed,
            ended: self.ended || other.ended,
        }
    }
}

/// The bindings the pass follows, each with a dense index.
struct Tracked {
    /// The index of each local of the body, if tracked.
    index: Vec<Option<usize>>,
    locals: Vec<Local>,
    /// Whether each tracked binding has a consuming end, by index: only
    /// those are elaborated.
    consumed: Vec<bool>,
}

impl Tracked {
    /// The bindings with a consuming end, which the pass elaborates, and the
    /// bindings ended at more than one place, which it checks are never
    /// ended twice along one path.
    // TODO: a parameter is never elaborated. Lowering writes no consuming end
    // for a parameter, so a resource parameter consumed on some paths and not
    // others keeps no flag, and the reference the body took over is released
    // where the function ends rather than where its new owner ends. Ending a
    // parameter's storage where it is consumed, and flagging it like a local,
    // would release it at the consuming site on every path.
    fn of(body: &Body) -> Tracked {
        let mut ends = vec![(0usize, false); body.local_decls.len()];
        for statement in body.basic_blocks.iter().flat_map(|block| &block.statements) {
            if let StatementKind::StorageDead(place, cause) = &statement.kind {
                if let Some(end) = ends
                    .get_mut(place.local.0)
                    .filter(|_| place.projection.is_empty())
                {
                    end.0 += 1;
                    end.1 |= *cause == EndCause::Consumed;
                }
            }
        }
        let mut tracked = Tracked {
            index: vec![None; body.local_decls.len()],
            locals: Vec::new(),
            consumed: Vec::new(),
        };
        for (local, (count, consumed)) in ends.into_iter().enumerate() {
            if consumed || count > 1 {
                tracked.index[local] = Some(tracked.locals.len());
                tracked.locals.push(Local(local));
                tracked.consumed.push(consumed);
            }
        }
        tracked
    }

    /// The tracked index of the whole local `place` names.
    fn of_place(&self, place: &Place) -> Option<usize> {
        if !place.projection.is_empty() {
            return None;
        }
        self.index.get(place.local.0).copied().flatten()
    }
}

/// What a statement does to the binding it names, if it is tracked.
fn statement_effect(tracked: &Tracked, kind: &StatementKind) -> Option<(usize, Held)> {
    match kind {
        StatementKind::StorageDead(place, EndCause::Scope) => {
            Some((tracked.of_place(place)?, Held::ENDED))
        }
        StatementKind::StorageDead(place, EndCause::Consumed) => {
            Some((tracked.of_place(place)?, Held::CONSUMED))
        }
        StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
            Some((tracked.of_place(place)?, Held::LIVE))
        }
        StatementKind::StorageLive(place) => Some((tracked.of_place(place)?, Held::default())),
        StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => None,
    }
}

/// The tracked bindings `terminator` binds whole: a call's result, an `out`
/// argument, a launch's destination.
fn bound_by<'a>(
    tracked: &'a Tracked,
    terminator: &'a Terminator,
) -> impl Iterator<Item = usize> + 'a {
    (terminator.written_places()).filter_map(|place| tracked.of_place(place))
}

/// The state at the end of `block`, entered in `state`.
fn run_block(body: &Body, tracked: &Tracked, block: usize, state: &mut [Held]) {
    let data = &body.basic_blocks[block];
    for statement in &data.statements {
        if let Some((binding, held)) = statement_effect(tracked, &statement.kind) {
            state[binding] = held;
        }
    }
    for binding in data.terminator.iter().flat_map(|t| bound_by(tracked, t)) {
        state[binding] = Held::LIVE;
    }
}

/// What every block is entered holding, for the blocks the entry reaches.
fn entry_states(body: &Body, tracked: &Tracked) -> Vec<Option<Vec<Held>>> {
    let blocks = body.basic_blocks.len();
    let successors: Vec<Vec<usize>> = (0..blocks).map(|block| successors(body, block)).collect();
    let mut entries: Vec<Option<Vec<Held>>> = vec![None; blocks];
    let mut queued = vec![false; blocks];
    let mut work = VecDeque::new();
    if blocks > 0 {
        entries[0] = Some(vec![Held::default(); tracked.locals.len()]);
        work.push_back(0);
        queued[0] = true;
    }
    while let Some(block) = work.pop_front() {
        queued[block] = false;
        let mut state = entries[block].clone().unwrap_or_default();
        run_block(body, tracked, block, &mut state);
        for &successor in &successors[block] {
            let changed = match &mut entries[successor] {
                Some(entry) => join_into(entry, &state),
                slot @ None => {
                    *slot = Some(state.clone());
                    true
                }
            };
            if changed && !queued[successor] {
                queued[successor] = true;
                work.push_back(successor);
            }
        }
    }
    entries
}

fn join_into(into: &mut [Held], from: &[Held]) -> bool {
    let mut changed = false;
    for (held, other) in into.iter_mut().zip(from) {
        let joined = held.join(*other);
        changed |= joined != *held;
        *held = joined;
    }
    changed
}

fn successors(body: &Body, block: usize) -> Vec<usize> {
    let terminator = body.basic_blocks[block].terminator.as_ref();
    (terminator
        .map(Terminator::successors)
        .unwrap_or_default()
        .into_iter())
    .map(|successor| successor.0)
    .filter(|&successor| successor < body.basic_blocks.len())
    .collect()
}

/// What the pass does to one end or rebinding of a tracked binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rewrite {
    /// An end every path into it has already passed: removed.
    Remove,
    /// An end some paths into it have already passed: kept behind the flag.
    Guard,
    /// A rebinding every path into it reaches with the binding ended: it
    /// releases no old value.
    Bind,
    /// A rebinding some paths reach with the binding ended: the old value is
    /// released behind the flag, then the binding takes the new one.
    GuardThenBind,
}

/// Each rewrite in a block, by statement index, with the binding it is of.
type BlockPlan = BTreeMap<usize, (Local, Rewrite)>;

/// The rewrite an end or rebinding of a binding entered in `held` needs, or
/// why the binding is ended twice along one path.
fn rewrite_of(kind: &StatementKind, held: Held) -> Result<Option<Rewrite>, &'static str> {
    match kind {
        StatementKind::StorageDead(_, cause) => end_rewrite(*cause, held),
        StatementKind::Reassign(..) => Ok(rebind_rewrite(held)),
        StatementKind::Assign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => Ok(None),
    }
}

/// The rewrite an end of a binding entered in `held` needs: only a scope end
/// some path reaches after a consuming end changes.
fn end_rewrite(cause: EndCause, held: Held) -> Result<Option<Rewrite>, &'static str> {
    if held.ended {
        return Err("its storage ends a second time along a path that never bound it again");
    }
    match cause {
        EndCause::Consumed if held.consumed => {
            Err("it is consumed a second time along a path that never bound it again")
        }
        EndCause::Consumed => Ok(None),
        EndCause::Scope if !held.consumed => Ok(None),
        EndCause::Scope if held.live => Ok(Some(Rewrite::Guard)),
        EndCause::Scope => Ok(Some(Rewrite::Remove)),
    }
}

/// The rewrite a rebinding of a binding entered in `held` needs: a path that
/// already ended it holds no old value to release.
fn rebind_rewrite(held: Held) -> Option<Rewrite> {
    if !(held.consumed || held.ended) {
        return None;
    }
    Some(if held.live {
        Rewrite::GuardThenBind
    } else {
        Rewrite::Bind
    })
}

/// The rewrites every reachable block needs, or a description of the first
/// end reached along a path that already ended the binding.
fn plan(
    body: &Body,
    tracked: &Tracked,
    entries: &[Option<Vec<Held>>],
) -> Result<Vec<BlockPlan>, String> {
    let mut plans = vec![BlockPlan::new(); body.basic_blocks.len()];
    for (block, entry) in entries.iter().enumerate() {
        let Some(mut state) = entry.clone() else {
            continue;
        };
        for (index, statement) in body.basic_blocks[block].statements.iter().enumerate() {
            let Some((binding, held)) = statement_effect(tracked, &statement.kind) else {
                continue;
            };
            let local = tracked.locals[binding];
            match rewrite_of(&statement.kind, state[binding]) {
                Ok(Some(rewrite)) if tracked.consumed[binding] => {
                    plans[block].insert(index, (local, rewrite));
                }
                Ok(Some(_) | None) => {}
                Err(why) => return Err(format!("_{} at bb{block}[{index}]: {why}", local.0)),
            }
            state[binding] = held;
        }
    }
    Ok(plans)
}

/// Release every binding a consuming site gives up exactly once on every
/// path: remove each scope end of it every path into which consumed it, and
/// guard each scope end some of those paths consumed it on by a drop flag.
///
/// Refuses a body that ends a binding twice along one path with no binding
/// between: lowering wrote a release it should not have, and no rewrite here
/// can tell which of the two it meant.
pub fn elaborate_drops(body: &mut Body) -> Result<(), String> {
    if body.is_gpu() {
        return Ok(());
    }
    let tracked = Tracked::of(body);
    if tracked.locals.is_empty() {
        return Ok(());
    }
    let entries = entry_states(body, &tracked);
    let plans = plan(body, &tracked, &entries)?;
    if plans.iter().all(BTreeMap::is_empty) {
        return Ok(());
    }
    let flags = allocate_flags(body, &plans);
    for (block, block_plan) in plans.iter().enumerate() {
        if !block_plan.is_empty() || !flags.is_empty() {
            rewrite_block(body, block, block_plan, &flags);
        }
    }
    set_flags_at_bindings_by_terminators(body, &flags);
    initialize_flags(body, &flags);
    body.cfg_changed();
    Ok(())
}

/// A drop flag for every binding some end of which is guarded, by binding,
/// declared in the order the guarded ends appear.
fn allocate_flags(body: &mut Body, plans: &[BlockPlan]) -> HashMap<Local, Local> {
    let mut flags = HashMap::new();
    for (binding, rewrite) in plans.iter().flat_map(BTreeMap::values) {
        if matches!(rewrite, Rewrite::Guard | Rewrite::GuardThenBind)
            && !flags.contains_key(binding)
        {
            let span = body.local_decls[binding.0].span;
            let mut decl = LocalDecl::new(Type::new(TypeKind::Boolean, span), span);
            decl.flag_of = Some(FlagOf::Drop(*binding));
            flags.insert(*binding, body.new_local(decl));
        }
    }
    flags
}

/// Apply `block_plan` to `block`, splitting it at each guarded end, and keep
/// each flagged binding's flag in step with the value it holds.
fn rewrite_block(
    body: &mut Body,
    block: usize,
    block_plan: &BlockPlan,
    flags: &HashMap<Local, Local>,
) {
    let statements = std::mem::take(&mut body.basic_blocks[block].statements);
    let terminator = body.basic_blocks[block].terminator.take();
    let mut current = BasicBlock(block);
    for (index, statement) in statements.into_iter().enumerate() {
        let span = statement.span;
        let statement = match block_plan.get(&index) {
            None => statement,
            Some((_, Rewrite::Remove)) => continue,
            Some((binding, Rewrite::Guard)) => {
                current = guard_end(body, current, *binding, flags, span);
                continue;
            }
            Some((_, Rewrite::Bind)) => bind(statement),
            Some((binding, Rewrite::GuardThenBind)) => {
                current = guard_end(body, current, *binding, flags, span);
                bind(statement)
            }
        };
        let flag_write = flag_write_after(&statement.kind, flags, span);
        body.basic_blocks[current.0].statements.push(statement);
        body.basic_blocks[current.0].statements.extend(flag_write);
    }
    body.basic_blocks[current.0].terminator = terminator;
}

/// `statement`, a rebinding, as a first binding: it releases no old value.
fn bind(statement: Statement) -> Statement {
    let kind = match statement.kind {
        StatementKind::Reassign(place, rvalue) => StatementKind::Assign(place, rvalue),
        kind @ (StatementKind::Assign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_, _)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_)) => kind,
    };
    Statement { kind, ..statement }
}

/// The flag write that keeps a flagged binding's flag in step after
/// `kind`: set where it takes a value, cleared where its storage ends.
fn flag_write_after(
    kind: &StatementKind,
    flags: &HashMap<Local, Local>,
    span: Span,
) -> Option<Statement> {
    let (place, value) = match kind {
        StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => (place, true),
        StatementKind::StorageDead(place, _) => (place, false),
        StatementKind::StorageLive(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => return None,
    };
    if !place.projection.is_empty() {
        return None;
    }
    let flag = flags.get(&place.local)?;
    Some(flag_assignment(*flag, value, span))
}

/// End `binding`'s storage behind a test of its flag, from block `from`, and
/// return the block that continues after it.
fn guard_end(
    body: &mut Body,
    from: BasicBlock,
    binding: Local,
    flags: &HashMap<Local, Local>,
    span: Span,
) -> BasicBlock {
    let Some(&flag) = flags.get(&binding) else {
        return from;
    };
    let release = new_block(body);
    let skip = new_block(body);
    let continuation = new_block(body);
    body.basic_blocks[from.0].terminator = Some(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(flag)),
            targets: vec![(Discriminant::bool_false(), skip)],
            otherwise: release,
        },
        span,
    ));
    body.basic_blocks[release.0].statements = vec![
        Statement {
            kind: StatementKind::StorageDead(Place::new(binding), EndCause::Scope),
            span,
        },
        flag_assignment(flag, false, span),
    ];
    for block in [release, skip] {
        body.basic_blocks[block.0].terminator = Some(goto(continuation, span));
    }
    continuation
}

/// Set a flagged binding's flag on every edge out of a terminator that binds
/// it whole ([`Terminator::written_places`]), through a block of its own.
fn set_flags_at_bindings_by_terminators(body: &mut Body, flags: &HashMap<Local, Local>) {
    let mut edges: Vec<(usize, BasicBlock, Vec<Local>, Span)> = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let Some(terminator) = data.terminator.as_ref() else {
            continue;
        };
        let set: Vec<Local> = (terminator.written_places())
            .filter(|place| place.projection.is_empty())
            .filter_map(|place| flags.get(&place.local).copied())
            .collect();
        if set.is_empty() {
            continue;
        }
        let mut targets = terminator.successors();
        targets.dedup();
        edges.extend(
            targets
                .into_iter()
                .map(|target| (block, target, set.clone(), terminator.span)),
        );
    }
    for (block, target, set, span) in edges {
        let edge = new_block(body);
        let writes = set
            .into_iter()
            .map(|flag| flag_assignment(flag, true, span));
        body.basic_blocks[edge.0].statements.extend(writes);
        body.basic_blocks[edge.0].terminator = Some(goto(target, span));
        if let Some(terminator) = body.basic_blocks[block].terminator.as_mut() {
            terminator.replace_successor(target, edge);
        }
    }
}

/// Clear every flag on entry: a binding holds no value before it is given one.
fn initialize_flags(body: &mut Body, flags: &HashMap<Local, Local>) {
    let span = body.span;
    let mut cleared: Vec<Local> = flags.values().copied().collect();
    cleared.sort_unstable_by_key(|flag| flag.0);
    let writes = cleared
        .into_iter()
        .map(|flag| flag_assignment(flag, false, span));
    body.basic_blocks[0].statements.splice(0..0, writes);
}

fn new_block(body: &mut Body) -> BasicBlock {
    body.basic_blocks.push(BasicBlockData::new(None));
    BasicBlock(body.basic_blocks.len() - 1)
}

fn goto(target: BasicBlock, span: Span) -> Terminator {
    Terminator::new(TerminatorKind::Goto { target }, span)
}

/// `flag = value`.
fn flag_assignment(flag: Local, value: bool, span: Span) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(flag),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: Literal::Boolean(value),
            }))),
        ),
        span,
    }
}
