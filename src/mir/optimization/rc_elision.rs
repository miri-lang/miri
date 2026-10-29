// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! RC elision pass: removes redundant IncRef/DecRef pairs.
//!
//! After the Perceus pass inserts RC operations, this pass removes pairs of
//! `IncRef(src)` + `DecRef(dest)` that are provably redundant. A pair is
//! redundant when `dest = Copy(src)` and neither the IncRef nor the DecRef
//! has any net effect on the program's memory safety:
//!
//! - The IncRef increments the RC to account for the copy alias.
//! - The DecRef decrements it back when the alias dies.
//! - Net effect: zero.
//!
//! # Safety conditions
//!
//! Elision of the pair `(IncRef(src), DecRef(dest))` is safe when:
//!
//! 1. Both operations are in the **same basic block** (scope-local).
//! 2. `src` does not have a user-defined destructor (`has_drop` types are skipped).
//!    Elision is safe only for types where RC reaching 0 has no side effects
//!    beyond freeing memory.
//! 3. `DecRef(src)` does not appear between the copy assignment and `DecRef(dest)`.
//!    If src's reference count reaches zero before dest's alias is cleaned up,
//!    dest would hold a dangling pointer after elision.
//!
//! # Example
//!
//! Before elision (after Perceus):
//! ```text
//! IncRef(_1)           // _1 is a List param
//! _3 = Copy(_1)
//! _4 = Copy(_3[0])     // index read — int, not managed
//! DecRef(_3)
//! StorageDead(_3)
//! ```
//!
//! After elision:
//! ```text
//! _3 = Copy(_1)
//! _4 = Copy(_3[0])
//! StorageDead(_3)      // no DecRef — _3 held no owned reference
//! ```
//!
//! The caller's RC for `_1` is unchanged; the object is freed by the caller.
//!
//! # Dying sources
//!
//! A copy of a local that dies right after it — `IncRef(x); d = x` or
//! `IncRef(x); d = Some(x)`, then `DecRef(x)` and the end of `x` — retains `x`
//! only for `x`'s own release to give the reference back. The pair cancels and
//! the copy becomes a move: the destination takes over `x`'s reference. See
//! [`move_dying_sources`].
//!
//! # Owned arguments
//!
//! A call passing an argument [`ArgMode::Owned`] is preceded by the retain that
//! funds it. When the local passed dies right after the call — its first
//! mention in the continuation is the release Perceus emitted for it, followed
//! by the end of its storage or a rebinding — the two cancel: the callee takes
//! over the caller's own reference instead of a second one, and a callee that
//! writes the value finds it unshared. See [`cancel_owned_transfers`].

use crate::mir::block::BasicBlock;
use crate::mir::optimization::OptimizationPass;
use crate::mir::place::{Local, PlaceContext};
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::types::MirType;
use crate::mir::visitor::Visitor;
use crate::mir::{Body, Operand, Place, Rvalue};
use std::collections::HashSet;

pub struct RcElision;

impl OptimizationPass for RcElision {
    fn run(&mut self, body: &mut Body) -> bool {
        let Body {
            basic_blocks,
            has_drop_types,
            local_decls,
            borrowed_locals,
            ..
        } = body;

        let lenders: HashSet<Local> = borrowed_locals.values().flatten().copied().collect();
        let borrows: HashSet<Local> = borrowed_locals.keys().copied().collect();
        let kept = KeptPairs {
            lenders: &lenders,
            borrows: &borrows,
        };
        let mut any_changed = false;
        for block in basic_blocks.iter_mut() {
            if elide_block_keeping(block, has_drop_types, local_decls, kept) {
                any_changed = true;
            }
        }
        let moved = move_dying_sources(body, kept);
        cancel_owned_transfers(body) || moved || any_changed
    }

    fn name(&self) -> &'static str {
        "RC Elision"
    }
}

/// Process one basic block, removing provably redundant (IncRef, DecRef) pairs.
///
/// Returns true if any statements were removed.
pub fn elide_block(
    block: &mut crate::mir::block::BasicBlockData,
    has_drop_types: &HashSet<String>,
    local_decls: &[crate::mir::LocalDecl],
) -> bool {
    let none = HashSet::new();
    let kept = KeptPairs {
        lenders: &none,
        borrows: &none,
    };
    elide_block_keeping(block, has_drop_types, local_decls, kept)
}

/// The pairs RC elision keeps for the borrowed locals of a body
/// ([`Body::borrowed_locals`]).
#[derive(Clone, Copy)]
struct KeptPairs<'a> {
    /// The locals some borrow relies on to keep what it holds alive. Copied
    /// into without a count, one would share its source's value, and a write
    /// to the source could free what the borrow holds.
    lenders: &'a HashSet<Local>,
    /// The borrowed locals. A pair copying one assumes the copy's source
    /// holds its value until it is released, but a borrow's value goes when
    /// its owner changes, with no release of the borrow to see.
    borrows: &'a HashSet<Local>,
}

/// [`elide_block`], keeping every pair `kept` names.
fn elide_block_keeping(
    block: &mut crate::mir::block::BasicBlockData,
    has_drop_types: &HashSet<String>,
    local_decls: &[crate::mir::LocalDecl],
    kept: KeptPairs,
) -> bool {
    let stmts = &block.statements;
    let n = stmts.len();

    // Positions to remove (IncRef and DecRef positions that form a redundant pair).
    let mut to_remove: HashSet<usize> = HashSet::new();

    // Scan for pattern:
    //   pos i:   IncRef(src)
    //   pos i+1: Assign(dest, Use(Copy(src)))  (no projections on either side)
    //   pos j:   DecRef(dest)                  (j > i+1, same block)
    // with safety conditions:
    //   - src's type has no destructor
    //   - no DecRef(src) in statements [i+2, j)
    for i in 0..n.saturating_sub(1) {
        // Already marked for removal — skip.
        if to_remove.contains(&i) {
            continue;
        }

        let Some((src_local, dest_local)) = retained_copy(stmts, i) else {
            continue;
        };

        // src's type must not carry a user-defined destructor, a borrow must
        // not rely on dest's reference, and src must hold its own.
        if type_has_destructor(src_local, local_decls, has_drop_types)
            || kept.lenders.contains(&dest_local)
            || kept.borrows.contains(&src_local)
        {
            continue;
        }

        // Find DecRef(dest_local) later in the block (first occurrence after i+1).
        let j = match find_decref(stmts, dest_local, i + 2) {
            Some(pos) => pos,
            None => continue,
        };

        // Safety check: no DecRef(src) in statements [i+2, j).
        if decref_in_range(stmts, src_local, i + 2, j) {
            continue;
        }

        to_remove.insert(i);
        to_remove.insert(j);
    }

    if to_remove.is_empty() {
        return false;
    }

    // Rebuild the statement list without the marked positions.
    let old_stmts = std::mem::take(&mut block.statements);
    block.statements = old_stmts
        .into_iter()
        .enumerate()
        .filter(|(pos, _)| !to_remove.contains(pos))
        .map(|(_, s)| s)
        .collect();

    true
}

/// The source and destination of the whole-local copy at `i + 1` when the
/// statement at `i` retains its source for it: `IncRef(src)` followed by
/// `dest = Copy(src)`, neither projected.
fn retained_copy(stmts: &[Statement], i: usize) -> Option<(Local, Local)> {
    let StatementKind::IncRef(retained) = &stmts.get(i)?.kind else {
        return None;
    };
    let StatementKind::Assign(dest, rvalue) = &stmts.get(i + 1)?.kind else {
        return None;
    };
    let source = copy_source(rvalue)?;
    let is_whole_copy = retained.projection.is_empty()
        && dest.projection.is_empty()
        && *source == Place::new(retained.local);
    is_whole_copy.then_some((retained.local, dest.local))
}

/// Turn each copy of a local that dies right after it into a move, cancelling
/// the retain that funded the copy against the local's own release.
///
/// The triple `IncRef(x)` … `d = x` (or `d` built from `x`) … `DecRef(x)` in
/// one block becomes `d = move x` when:
///
/// 1. `x` is a whole local the body owns and releases — managed, not a
///    parameter or a capture, holding no device buffer — that no borrow reads
///    and that borrows from nothing;
/// 2. the first mention of `x` after the retain is an assignment or rebinding
///    whose value is `x` itself or an aggregate naming `x` once, and whose
///    destination does not name `x`;
/// 3. the next mention of `x` is `DecRef(x)`, with nothing released between —
///    so the release the pair cancels was never the last one, and no drop hook
///    moves — followed, with no mention of `x` between, by `StorageDead(x)` or
///    a rebinding of `x` that does not read it.
///
/// Between the copy and that end `x` holds a reference the destination now
/// owns, and nothing reads it.
///
/// Returns true if any copy became a move.
fn move_dying_sources(body: &mut Body, kept: KeptPairs) -> bool {
    let mut changed = false;
    for block in 0..body.basic_blocks.len() {
        while let Some(transfer) = dying_source_transfer(body, block, kept) {
            apply_transfer(&mut body.basic_blocks[block].statements, transfer);
            changed = true;
        }
    }
    changed
}

/// The positions of one retain, the copy it funds and the release it cancels
/// against, for a copy of a local dying right after it.
#[derive(Clone, Copy)]
struct DyingTransfer {
    local: Local,
    retain: usize,
    copy: usize,
    release: usize,
}

/// The first copy in `block` of `body` that [`move_dying_sources`] turns into
/// a move.
fn dying_source_transfer(body: &Body, block: usize, kept: KeptPairs) -> Option<DyingTransfer> {
    let statements = &body.basic_blocks[block].statements;
    statements
        .iter()
        .enumerate()
        .find_map(|(retain, statement)| {
            let StatementKind::IncRef(retained) = &statement.kind else {
                return None;
            };
            let local = retained.projection.is_empty().then_some(retained.local)?;
            if !is_transferable_source(body, local, kept) {
                return None;
            }
            let copy = next_mention(statements, local, retain + 1)?;
            if !transfers_local(&statements[copy], local) {
                return None;
            }
            let release = copy + 1 + dying_release(&statements[copy + 1..], local)?;
            let releases_between = statements[copy + 1..release].iter().any(|statement| {
                matches!(
                    statement.kind,
                    StatementKind::DecRef(_) | StatementKind::Dealloc(_)
                )
            });
            (!releases_between).then_some(DyingTransfer {
                local,
                retain,
                copy,
                release,
            })
        })
}

/// Whether `local` is one the body owns and releases, and no borrow involves.
fn is_transferable_source(body: &Body, local: Local, kept: KeptPairs) -> bool {
    local.0 > body.arg_count
        && !body.env_capture_locals.contains(&local)
        && !kept.borrows.contains(&local)
        && !kept.lenders.contains(&local)
        && body.local_decls[local.0].device_handle.is_none()
        && body.is_counted_local(local)
}

/// The position of the first statement at or after `start` mentioning `local`.
fn next_mention(statements: &[Statement], local: Local, start: usize) -> Option<usize> {
    (start..statements.len()).find(|&at| statement_mentions(&statements[at], local))
}

/// Whether `statement` stores `local` whole into a destination not naming it:
/// as the value itself, or as one operand of an aggregate naming it once.
fn transfers_local(statement: &Statement, local: Local) -> bool {
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return false;
    };
    let names_dest = mentions(local, |mention| {
        mention.visit_place(dest, PlaceContext::MutatingUse, BasicBlock(0))
    });
    let mut naming = stored_operands(rvalue).iter().filter(|operand| {
        mentions(local, |mention| {
            mention.visit_operand(operand, BasicBlock(0))
        })
    });
    let is_stored_whole_once = naming
        .next()
        .is_some_and(|operand| operand.bare_local() == Some(local))
        && naming.next().is_none();
    is_stored_whole_once && !names_dest
}

/// Remove the retain and the release of `transfer` and spell its copy as a
/// move of the local.
fn apply_transfer(statements: &mut Vec<Statement>, transfer: DyingTransfer) {
    if let StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) =
        &mut statements[transfer.copy].kind
    {
        for operand in stored_operands_mut(rvalue) {
            if operand.bare_local() == Some(transfer.local) {
                *operand = Operand::Move(Place::new(transfer.local));
            }
        }
    }
    statements.remove(transfer.release);
    statements.remove(transfer.retain);
}

/// The operands an rvalue stores into its destination as they are: the value
/// of a use, or every operand of an aggregate. Every other rvalue computes a
/// value of its own from what it reads.
fn stored_operands(rvalue: &Rvalue) -> &[Operand] {
    match rvalue {
        Rvalue::Use(operand) => std::slice::from_ref(operand),
        Rvalue::Aggregate(_, operands) => operands,
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => &[],
    }
}

/// [`stored_operands`], to rewrite.
fn stored_operands_mut(rvalue: &mut Rvalue) -> &mut [Operand] {
    match rvalue {
        Rvalue::Use(operand) => std::slice::from_mut(operand),
        Rvalue::Aggregate(_, operands) => operands,
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => &mut [],
    }
}

/// Cancel each retain funding an owned argument against the caller's release
/// of the same local right after the call, so the callee takes over the
/// caller's own reference.
///
/// The pair `IncRef(L)` … `call f(L)` → `DecRef(L)` is cancelled when:
///
/// 1. `L` is a whole local the body owns and releases — managed, not a
///    parameter or a capture, holding no device buffer — whose type has no drop
///    hook;
/// 2. the call passes `L` once, at an [`ArgMode::Owned`] position, and names it
///    nowhere else: in another argument the callee would read a value the
///    owned side may already have freed or written;
/// 3. the retain is the last mention of `L` before the call;
/// 4. the call's continuation is reached from the call alone — never the
///    entry block, which the edge into the body also reaches — and its first
///    mention of `L` is `DecRef(L)`, followed — with no mention of `L` between —
///    by `StorageDead(L)` or a rebinding of `L` that does not read it.
///
/// Between the call and that end `L` holds a reference the callee may have
/// released, and nothing reads it.
///
/// Returns true if any pair was cancelled.
pub fn cancel_owned_transfers(body: &mut Body) -> bool {
    if !passes_an_owned_argument(body) {
        return false;
    }
    let predecessors = body.predecessor_counts();
    let mut cancelled: Vec<(usize, usize)> = (0..body.basic_blocks.len())
        .flat_map(|block| owned_transfers_from(body, block, &predecessors))
        .collect();
    cancelled.sort_unstable();
    cancelled.dedup();
    for (block, position) in cancelled.iter().rev() {
        body.basic_blocks[*block].statements.remove(*position);
    }
    !cancelled.is_empty()
}

/// The (block, statement) positions of every retain and release the call
/// ending `block` lets cancel.
fn owned_transfers_from(body: &Body, block: usize, predecessors: &[usize]) -> Vec<(usize, usize)> {
    let data = &body.basic_blocks[block];
    let Some(TerminatorKind::Call {
        args,
        arg_modes,
        destination,
        target: Some(target),
        ..
    }) = data.terminator.as_ref().map(|terminator| &terminator.kind)
    else {
        return Vec::new();
    };
    if target.0 == block || predecessors[target.0] != 1 {
        return Vec::new();
    }
    let mut retains = Vec::new();
    let mut releases = Vec::new();
    for (position, arg) in args.iter().enumerate() {
        if arg_mode(arg_modes, position) != ArgMode::Owned {
            continue;
        }
        let Some(local) = arg.bare_local() else {
            continue;
        };
        let is_passed_once = args
            .iter()
            .filter(|other| mentions(local, |mention| mention.visit_operand(other, BasicBlock(0))))
            .count()
            == 1;
        let receives_the_result = mentions(local, |mention| {
            mention.visit_place(destination, PlaceContext::MutatingUse, BasicBlock(0))
        });
        if !is_passed_once || receives_the_result || !is_released_by_body(body, local) {
            continue;
        }
        let continuation = &body.basic_blocks[target.0].statements;
        if let (Some(retain), Some(release)) = (
            funding_retain(&data.statements, local),
            dying_release(continuation, local),
        ) {
            retains.push((block, retain));
            releases.push((target.0, release));
        }
    }
    retains.into_iter().chain(releases).collect()
}

/// Whether any call in `body` passes an argument [`ArgMode::Owned`].
fn passes_an_owned_argument(body: &Body) -> bool {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .any(|terminator| match &terminator.kind {
            TerminatorKind::Call { arg_modes, .. } => arg_modes.contains(&ArgMode::Owned),
            TerminatorKind::VirtualCall { .. }
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => false,
        })
}

/// Whether `local` is one the body owns and Perceus releases, whose last
/// release runs no drop hook. A borrowed local is never released.
fn is_released_by_body(body: &Body, local: Local) -> bool {
    let decl = &body.local_decls[local.0];
    local.0 > body.arg_count
        && !body.env_capture_locals.contains(&local)
        && !body.borrowed_locals.contains_key(&local)
        && decl.device_handle.is_none()
        && body.is_counted_local(local)
        && !type_has_destructor(local, &body.local_decls, &body.has_drop_types)
}

/// The position of `IncRef(local)` when it is the block's last mention of
/// `local`.
fn funding_retain(statements: &[Statement], local: Local) -> Option<usize> {
    let last = statements
        .iter()
        .rposition(|statement| statement_mentions(statement, local))?;
    matches!(&statements[last].kind, StatementKind::IncRef(p) if *p == Place::new(local))
        .then_some(last)
}

/// The position of `DecRef(local)` when it is the block's first mention of
/// `local` and is followed, with no mention between, by the end of the local's
/// storage or a rebinding of it that does not read it.
fn dying_release(statements: &[Statement], local: Local) -> Option<usize> {
    let whole = Place::new(local);
    let mut mentioning = statements
        .iter()
        .enumerate()
        .filter(|(_, statement)| statement_mentions(statement, local));
    let (release, first) = mentioning.next()?;
    if !matches!(&first.kind, StatementKind::DecRef(p) if *p == whole) {
        return None;
    }
    let (_, next) = mentioning.next()?;
    let ends_value = match &next.kind {
        StatementKind::StorageDead(p) => *p == whole,
        StatementKind::Reassign(p, rvalue) => {
            *p == whole && !mentions(local, |mention| mention.visit_rvalue(rvalue, BasicBlock(0)))
        }
        StatementKind::Assign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => false,
    };
    ends_value.then_some(release)
}

/// Finds whether a MIR fragment names a local, as the place it reads or writes
/// or as an index into another.
struct LocalMention {
    local: Local,
    found: bool,
}

impl Visitor for LocalMention {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.found |= local == self.local;
    }
}

/// Whether whatever `visit` walks mentions `local`.
fn mentions(local: Local, visit: impl FnOnce(&mut LocalMention)) -> bool {
    let mut mention = LocalMention {
        local,
        found: false,
    };
    visit(&mut mention);
    mention.found
}

fn statement_mentions(statement: &Statement, local: Local) -> bool {
    mentions(local, |mention| {
        mention.visit_statement(BasicBlock(0), statement)
    })
}

/// Extract the source place if the rvalue is `Use(Copy(place))`.
fn copy_source(rvalue: &crate::mir::Rvalue) -> Option<&Place> {
    match rvalue {
        crate::mir::Rvalue::Use(Operand::Copy(p)) => Some(p),
        _ => None,
    }
}

/// Returns true if `local`'s MIR type is a Custom type with a user destructor.
fn type_has_destructor(
    local: Local,
    local_decls: &[crate::mir::LocalDecl],
    has_drop_types: &HashSet<String>,
) -> bool {
    match &local_decls[local.0].mir_ty {
        MirType::Custom(name) => has_drop_types.contains(name.as_str()),
        _ => false,
    }
}

/// Find the first `DecRef(local)` at or after `start_pos`, with no projection.
pub fn find_decref(
    stmts: &[crate::mir::Statement],
    local: Local,
    start_pos: usize,
) -> Option<usize> {
    for (i, stmt) in stmts.iter().enumerate().skip(start_pos) {
        if let StatementKind::DecRef(p) = &stmt.kind {
            if p.local == local && p.projection.is_empty() {
                return Some(i);
            }
        }
    }
    None
}

/// Returns true if `DecRef(local)` appears in statements `[start, end)`.
pub fn decref_in_range(
    stmts: &[crate::mir::Statement],
    local: Local,
    start: usize,
    end: usize,
) -> bool {
    stmts[start..end.min(stmts.len())]
        .iter()
        .any(|stmt| matches!(&stmt.kind, StatementKind::DecRef(p) if p.local == local && p.projection.is_empty()))
}

/// Count IncRef and DecRef operations for a specific local across all blocks.
///
/// Used by tests to assert that RC operations were eliminated.
pub fn count_rc_ops(body: &Body, local: Local) -> (usize, usize) {
    let mut incref = 0usize;
    let mut decref = 0usize;
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::IncRef(p) if p.local == local => incref += 1,
                StatementKind::DecRef(p) if p.local == local => decref += 1,
                _ => {}
            }
        }
    }
    (incref, decref)
}

/// Count ALL IncRef and DecRef operations across all blocks in the body.
///
/// Used by tests to verify that a linear program has zero RC operations.
pub fn count_all_rc_ops(body: &Body) -> (usize, usize) {
    let mut incref = 0usize;
    let mut decref = 0usize;
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::IncRef(_) => incref += 1,
                StatementKind::DecRef(_) => decref += 1,
                _ => {}
            }
        }
    }
    (incref, decref)
}
