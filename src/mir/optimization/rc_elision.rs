// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! RC elision pass: removes redundant IncRef/DecRef pairs.
//!
//! After the RC insertion pass inserts RC operations, this pass removes pairs of
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
//! Before elision (after RC insertion):
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
//! A call passing an argument [`crate::mir::ArgMode::Owned`], or [`crate::mir::ArgMode::Donated`] to a
//! container that keeps it, is preceded by the retain that funds it. When the
//! local passed dies right after the call — its first mention in the
//! continuation is the release RC insertion emitted for it, followed by the end
//! of its storage or a rebinding — the two cancel: the callee or the container
//! takes over the caller's own reference instead of a second one, and a callee
//! that writes the value finds it unshared. See [`cancel_owned_transfers`].

use crate::mir::block::BasicBlock;
use crate::mir::facts::OwnershipFacts;
use crate::mir::option_layout::is_nullable_pointer_payload;
use crate::mir::place::{Local, PlaceContext};
use crate::mir::place_type::PlaceTypes;
use crate::mir::rc::RcClassifier;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::type_facts::DropHookReach;
use crate::mir::types::MirType;
use crate::mir::visitor::Visitor;
use crate::mir::{Body, Operand, Place, Rvalue};
use std::collections::HashSet;

pub struct RcElision;

impl RcElision {
    /// Removes the redundant retain and release pairs of `body`, keeping
    /// every pair the borrows `ownership` records rely on. Returns true if
    /// any statement was removed.
    pub(super) fn elide(&self, body: &mut Body, ownership: &OwnershipFacts) -> bool {
        let Body {
            basic_blocks,
            rc_classifier,
            local_decls,
            ..
        } = body;

        let borrowed_locals = &ownership.borrowed_locals;
        let lenders: HashSet<Local> = borrowed_locals.values().flatten().copied().collect();
        let borrows: HashSet<Local> = borrowed_locals.keys().copied().collect();
        let kept = KeptPairs {
            lenders: &lenders,
            borrows: &borrows,
        };
        let mut any_changed = false;
        for block in basic_blocks.iter_mut() {
            if elide_block_keeping(block, rc_classifier, local_decls, kept) {
                any_changed = true;
            }
        }
        let moved = move_dying_sources(body, kept);
        cancel_owned_transfers(body, ownership) || moved || any_changed
    }
}

/// Process one basic block, removing provably redundant (IncRef, DecRef) pairs.
///
/// Returns true if any statements were removed.
pub fn elide_block(
    block: &mut crate::mir::block::BasicBlockData,
    classifier: &RcClassifier,
    local_decls: &[crate::mir::LocalDecl],
) -> bool {
    let none = HashSet::new();
    let kept = KeptPairs {
        lenders: &none,
        borrows: &none,
    };
    elide_block_keeping(block, classifier, local_decls, kept)
}

/// The pairs RC elision keeps for the borrowed locals of a body
/// ([`OwnershipFacts::borrowed_locals`]).
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
    classifier: &RcClassifier,
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
        if type_has_destructor(src_local, local_decls, classifier)
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
/// one block — or, for an `x` whose release may run a drop hook, in a block
/// and the blocks it runs straight into, each reached only from the one
/// before it through a terminator not naming `x` — becomes `d = move x`
/// when:
///
/// 1. `x` is a whole local the body owns and releases — managed, not a
///    parameter or a capture, holding no device buffer — that no borrow reads
///    and that borrows from nothing;
/// 2. the first mention of `x` after the retain, past retains of places under
///    `x` funding the aggregate's other operands, is an assignment or
///    rebinding whose value is `x` itself or an aggregate holding `x` once,
///    every other operand naming `x` reading a place under it, and whose
///    destination does not name `x`: an aggregate reads every operand before
///    anything is released, so a sibling read of a field sees the value the
///    destination takes over;
/// 3. the next mention of `x` is `DecRef(x)`, with no release of the
///    destination between — so the release the pair cancels was never the
///    last one, and the destination still holds `x`'s reference when `x` dies:
///    a release of another place between gives up a reference of its own —
///    followed, with no mention of `x` between, by `StorageDead(x)` or
///    a rebinding of `x` that does not read it.
///
/// Between the copy and that end `x` holds a reference the destination now
/// owns, and nothing reads it.
///
/// Returns true if any copy became a move.
fn move_dying_sources(body: &mut Body, kept: KeptPairs) -> bool {
    let predecessors = crate::mir::analysis::loops::predecessors(body);
    let mut changed = false;
    for block in 0..body.basic_blocks.len() {
        while let Some(transfer) = dying_source_transfer(body, block, kept, &predecessors) {
            apply_transfer(body, block, transfer);
            changed = true;
        }
    }
    changed
}

/// A statement's position: its block and its index in the block.
type At = (usize, usize);

/// The positions of one retain, the copy it funds and the release it cancels
/// against, for a copy of a local dying right after it. The retain is in the
/// block searched; the copy and the release may be in the blocks it runs
/// straight into.
#[derive(Clone)]
struct DyingTransfer {
    /// The place the copy reads: the local, or the payload of an optional
    /// held as its payload's own pointer.
    source: Place,
    retain: usize,
    copy: At,
    release: At,
}

/// The statements from the one at `start` in `block` on, followed, given the
/// body's `predecessors`, by those of each block control runs straight into:
/// a block's terminator naming nothing of `local` and leading only to a block
/// nothing else leads to. A call returns to the one block it names, and
/// nothing it runs can reach a local it is not handed, so `local` holds what
/// it held across it.
fn straight_run<'b>(
    body: &'b Body,
    block: usize,
    start: usize,
    local: Local,
    predecessors: Option<&[Vec<usize>]>,
) -> Vec<(At, &'b Statement)> {
    let mut run = Vec::new();
    let mut current = block;
    let mut from = start;
    loop {
        let data = &body.basic_blocks[current];
        run.extend(
            (data.statements.iter().enumerate().skip(from)).map(|(at, s)| ((current, at), s)),
        );
        let (Some(terminator), Some(predecessors)) = (&data.terminator, predecessors) else {
            return run;
        };
        let successors = terminator.successors();
        let [next] = successors.as_slice() else {
            return run;
        };
        let names_local = mentions(local, |mention| {
            mention.visit_terminator(BasicBlock(current), terminator)
        });
        let is_straight = predecessors
            .get(next.0)
            .is_some_and(|p| p.as_slice() == [current])
            && next.0 != block
            && !body.basic_blocks[next.0].is_cleanup;
        if names_local || !is_straight {
            return run;
        }
        current = next.0;
        from = 0;
    }
}

/// The first copy in `block` of `body` that [`move_dying_sources`] turns into
/// a move.
fn dying_source_transfer(
    body: &Body,
    block: usize,
    kept: KeptPairs,
    predecessors: &[Vec<usize>],
) -> Option<DyingTransfer> {
    let statements = &body.basic_blocks[block].statements;
    statements
        .iter()
        .enumerate()
        .find_map(|(retain, statement)| {
            let StatementKind::IncRef(retained) = &statement.kind else {
                return None;
            };
            let local = released_with(body, retained)?;
            if !is_transferable_source(body, local, kept) {
                return None;
            }
            // Only a value whose release may run a drop hook is followed past
            // its block: every other count stays cancelled inside one block,
            // which the loop header reads rely on.
            let crosses = DropHookReach::new(body.rc_classifier.definitions())
                .may_run_drop_hook(&body.local_decls[local.0].ty);
            let run = straight_run(
                body,
                block,
                retain + 1,
                local,
                crosses.then_some(predecessors),
            );
            let run_statements: Vec<&Statement> = run.iter().map(|&(_, s)| s).collect();
            let copy = next_copy_mention(&run_statements, retained)?;
            if !transfers_local(run_statements[copy], retained) {
                return None;
            }
            let release = copy + 1 + dying_release(&run_statements[copy + 1..], local)?;
            let destination = stored_destination(run_statements[copy])?;
            let releases_between = run_statements[copy + 1..release].iter().any(|statement| {
                matches!(
                    &statement.kind,
                    StatementKind::DecRef(place) | StatementKind::Dealloc(place)
                        if place.local == destination
                )
            });
            (!releases_between).then(|| DyingTransfer {
                source: retained.clone(),
                retain,
                copy: run[copy].0,
                release: run[release].0,
            })
        })
}

/// The local whose release a retain of `retained` cancels against: the
/// local itself, retained whole, or an optional held as its payload's own
/// pointer whose payload is retained, since releasing that optional releases
/// exactly its payload.
fn released_with(body: &Body, retained: &Place) -> Option<Local> {
    (retained.projection.is_empty() || is_nullable_pointer_payload(retained, body))
        .then_some(retained.local)
}

/// Whether `local` is one the body owns and releases, and no borrow involves.
fn is_transferable_source(body: &Body, local: Local, kept: KeptPairs) -> bool {
    local.0 > body.arg_count
        && !body.captures.env_capture_locals.contains(&local)
        && !kept.borrows.contains(&local)
        && !kept.lenders.contains(&local)
        && body.local_decls[local.0].device_handle.is_none()
        && body.is_counted_local(local)
}

/// The position of the first statement in `statements` mentioning the local
/// of `source` other than a retain of a place under `source`.
fn next_copy_mention(statements: &[&Statement], source: &Place) -> Option<usize> {
    (0..statements.len()).find(|&at| {
        statement_mentions(statements[at], source.local) && !retains_under(statements[at], source)
    })
}

/// Whether `statement` retains a place under `owner`.
fn retains_under(statement: &Statement, owner: &Place) -> bool {
    matches!(&statement.kind, StatementKind::IncRef(place) if is_under(place, owner))
}

/// Whether `place` is a place strictly under `owner`: a field, an element or
/// a dereference reached through it.
fn is_under(place: &Place, owner: &Place) -> bool {
    place.local == owner.local
        && place.projection.len() > owner.projection.len()
        && place.projection.starts_with(&owner.projection)
}

/// Whether `operand` reads a place strictly under `owner`.
fn reads_under(operand: &Operand, owner: &Place) -> bool {
    match operand {
        Operand::Copy(read) => is_under(read, owner),
        Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => false,
    }
}

/// Whether `statement` stores `source` whole into a destination not naming
/// its local: as the value itself, or as one operand of an aggregate holding
/// it once, every other operand naming the local reading a place under it.
fn transfers_local(statement: &Statement, source: &Place) -> bool {
    let local = source.local;
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return false;
    };
    let names_dest = mentions(local, |mention| {
        mention.visit_place(dest, PlaceContext::MutatingUse, BasicBlock(0))
    });
    let naming = stored_operands(rvalue).iter().filter(|operand| {
        mentions(local, |mention| {
            mention.visit_operand(operand, BasicBlock(0))
        })
    });
    let (mut whole, mut siblings_under) = (0, true);
    for operand in naming {
        if reads_place(operand, source) {
            whole += 1;
        } else {
            siblings_under &= reads_under(operand, source);
        }
    }
    whole == 1 && siblings_under && !names_dest
}

/// Whether `operand` reads `place` as it is.
fn reads_place(operand: &Operand, place: &Place) -> bool {
    match operand {
        Operand::Copy(read) | Operand::Move(read) => read == place,
        Operand::Constant(_) | Operand::Function(_) => false,
    }
}

/// The local an assignment stores into.
fn stored_destination(statement: &Statement) -> Option<Local> {
    match &statement.kind {
        StatementKind::Assign(dest, _) | StatementKind::Reassign(dest, _) => Some(dest.local),
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(..)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::ReadHeader(_)
        | StatementKind::Nop => None,
    }
}

/// Remove the retain and the release of `transfer` and spell its copy as a
/// move of the local.
fn apply_transfer(body: &mut Body, block: usize, transfer: DyingTransfer) {
    let (copy_block, copy) = transfer.copy;
    if let StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) =
        &mut body.basic_blocks[copy_block].statements[copy].kind
    {
        for operand in stored_operands_mut(rvalue) {
            if reads_place(operand, &transfer.source) {
                *operand = Operand::Move(transfer.source.clone());
            }
        }
    }
    let (release_block, release) = transfer.release;
    body.basic_blocks[release_block].statements.remove(release);
    body.basic_blocks[block].statements.remove(transfer.retain);
}

/// The operands an rvalue stores into its destination as they are: the value
/// of a use, every operand of an aggregate, or the value a cast hands on — a
/// counted value cast to a trait or a base class is the same block, and a
/// cast of anything else is never retained, so never pairs. Every other
/// rvalue computes a value of its own from what it reads.
fn stored_operands(rvalue: &Rvalue) -> &[Operand] {
    match rvalue {
        Rvalue::Use(operand) => std::slice::from_ref(operand),
        Rvalue::Cast(operand, _) => std::slice::from_ref(operand.as_ref()),
        Rvalue::Aggregate(_, operands) => operands,
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. } => &[],
    }
}

/// [`stored_operands`], to rewrite.
fn stored_operands_mut(rvalue: &mut Rvalue) -> &mut [Operand] {
    match rvalue {
        Rvalue::Use(operand) => std::slice::from_mut(operand),
        Rvalue::Cast(operand, _) => std::slice::from_mut(operand.as_mut()),
        Rvalue::Aggregate(_, operands) => operands,
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. } => &mut [],
    }
}

/// Cancel each retain funding an owned argument against the caller's release
/// of the same local right after the call, so the callee takes over the
/// caller's own reference.
///
/// The pair `IncRef(L)` … `call f(L)` → `DecRef(L)` is cancelled when:
///
/// 1. `L` is a whole local the body owns and releases — managed, not a
///    parameter or a capture, holding no device buffer. A resource is no
///    exception: the callee is its one owner after the call, so its hook runs
///    where the callee lets it go;
/// 2. the call passes `L` once, at a position it hands over, and names it
///    nowhere else but in arguments reading a place under it: the call reads
///    every argument before the callee runs, so a field it passes beside `L`
///    is the value `L` held. Such a field must not be counted unless the call
///    hands its own reference over too, funded by a retain of its own: a
///    counted field the callee only borrows is a value the callee may free
///    by releasing `L` while it still reads it;
/// 3. the retain is the last mention of `L` before the call;
/// 4. the call's continuation is reached from the call alone — never the
///    entry block, which the edge into the body also reaches — and its first
///    mention of `L` is `DecRef(L)`, followed — with no mention of `L` between —
///    by `StorageDead(L)` or a rebinding of `L` that does not read it.
///
/// Between the call and that end `L` holds a reference the callee may have
/// released, and nothing reads it. The argument is spelled `move L` in the
/// same rewrite, so the hand-over of `L`'s own reference is written on the
/// call and the `moved_out` verifier refuses any later read or release of it.
///
/// Returns true if any pair was cancelled.
pub fn cancel_owned_transfers(body: &mut Body, ownership: &OwnershipFacts) -> bool {
    if !passes_an_owned_argument(body) {
        return false;
    }
    let predecessors = body.predecessor_counts();
    let transfers: Vec<OwnedTransfer> = (0..body.basic_blocks.len())
        .flat_map(|block| owned_transfers_from(body, ownership, block, &predecessors))
        .collect();
    let mut cancelled: Vec<(usize, usize)> = transfers
        .iter()
        .flat_map(|transfer| [transfer.retain, transfer.release])
        .collect();
    cancelled.sort_unstable();
    cancelled.dedup();
    for (block, position) in cancelled.iter().rev() {
        body.basic_blocks[*block].statements.remove(*position);
    }
    for transfer in &transfers {
        spell_as_move(body, transfer);
    }
    !transfers.is_empty()
}

/// One owned argument whose funding retain and dying release cancel.
struct OwnedTransfer {
    /// The block whose call hands the argument over.
    block: usize,
    /// The argument's position in the call.
    position: usize,
    /// The (block, statement) of the retain funding it.
    retain: (usize, usize),
    /// The (block, statement) of the release it dies at.
    release: (usize, usize),
}

/// Spell the argument `transfer` cancels as a move of the local it passes.
fn spell_as_move(body: &mut Body, transfer: &OwnedTransfer) {
    let terminator = body.basic_blocks[transfer.block].terminator.as_mut();
    if let Some(TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. }) =
        terminator.map(|terminator| &mut terminator.kind)
    {
        if let Some(argument) = args.get_mut(transfer.position) {
            if let Some(local) = argument.bare_local() {
                *argument = Operand::Move(Place::new(local));
            }
        }
    }
}

/// Every owned argument of the call ending `block` whose retain and release
/// cancel.
fn owned_transfers_from(
    body: &Body,
    ownership: &OwnershipFacts,
    block: usize,
    predecessors: &[usize],
) -> Vec<OwnedTransfer> {
    let data = &body.basic_blocks[block];
    let Some(
        TerminatorKind::Call {
            args,
            arg_modes,
            destination,
            target: Some(target),
            ..
        }
        | TerminatorKind::VirtualCall {
            args,
            arg_modes,
            destination,
            target: Some(target),
            ..
        },
    ) = data.terminator.as_ref().map(|terminator| &terminator.kind)
    else {
        return Vec::new();
    };
    if target.0 == block || predecessors[target.0] != 1 {
        return Vec::new();
    }
    let mut transfers = Vec::new();
    for (position, arg) in args.iter().enumerate() {
        if !arg_mode(arg_modes, position).hands_over_reference() {
            continue;
        }
        let Some(local) = arg.bare_local() else {
            continue;
        };
        let is_passed_once = is_passed_once_beside_fields(body, args, arg_modes, local);
        let receives_the_result = mentions(local, |mention| {
            mention.visit_place(destination, PlaceContext::MutatingUse, BasicBlock(0))
        });
        if !is_passed_once || receives_the_result || !is_released_by_body(body, ownership, local) {
            continue;
        }
        let continuation = &body.basic_blocks[target.0].statements;
        if let (Some(retain), Some(release)) = (
            funding_retain(&data.statements, local),
            dying_release(continuation, local),
        ) {
            transfers.push(OwnedTransfer {
                block,
                position,
                retain: (block, retain),
                release: (target.0, release),
            });
        }
    }
    transfers
}

/// Whether a call passing `args` with `arg_modes` passes `local` whole once,
/// and every other argument naming it reads a place under it that is not
/// counted or that the call hands over as well.
fn is_passed_once_beside_fields(
    body: &Body,
    args: &[Operand],
    arg_modes: &[ArgMode],
    local: Local,
) -> bool {
    let owner = Place::new(local);
    let place_types = PlaceTypes::of(body);
    let mut whole = 0;
    for (position, arg) in args.iter().enumerate() {
        if !mentions(local, |mention| mention.visit_operand(arg, BasicBlock(0))) {
            continue;
        }
        if reads_place(arg, &owner) {
            whole += 1;
            continue;
        }
        let Operand::Copy(field) = arg else {
            return false;
        };
        let is_counted = place_types
            .declared(field)
            .is_none_or(|ty| body.is_counted(&ty.kind));
        let is_safe_field = is_under(field, &owner)
            && (!is_counted || arg_mode(arg_modes, position).hands_over_reference());
        if !is_safe_field {
            return false;
        }
    }
    whole == 1
}

/// Whether any call in `body` hands an argument over, passing it
/// [`crate::mir::ArgMode::Owned`] or [`crate::mir::ArgMode::Donated`].
fn passes_an_owned_argument(body: &Body) -> bool {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| terminator.passed_arguments())
        .any(|(_, arg_modes)| arg_modes.iter().any(|mode| mode.hands_over_reference()))
}

/// Whether `local` is one the body owns and RC insertion releases. A
/// borrowed local is never released.
fn is_released_by_body(body: &Body, ownership: &OwnershipFacts, local: Local) -> bool {
    let decl = &body.local_decls[local.0];
    local.0 > body.arg_count
        && !body.captures.env_capture_locals.contains(&local)
        && !ownership.is_borrowed(local)
        && decl.device_handle.is_none()
        && body.is_counted_local(local)
}

/// The position of `IncRef(local)` when it is the block's last mention of
/// `local` other than a retain of a place under it, which funds a field the
/// call passes beside it.
fn funding_retain(statements: &[Statement], local: Local) -> Option<usize> {
    let whole = Place::new(local);
    let last = statements.iter().rposition(|statement| {
        statement_mentions(statement, local) && !retains_under(statement, &whole)
    })?;
    matches!(&statements[last].kind, StatementKind::IncRef(p) if *p == whole).then_some(last)
}

/// The position of `DecRef(local)` when it is the block's first mention of
/// `local` and is followed, with no mention between, by the end of the local's
/// storage or a rebinding of it that does not read it.
fn dying_release<S: std::borrow::Borrow<Statement>>(
    statements: &[S],
    local: Local,
) -> Option<usize> {
    let whole = Place::new(local);
    let mut mentioning = statements
        .iter()
        .map(|statement| statement.borrow())
        .enumerate()
        .filter(|(_, statement)| statement_mentions(statement, local));
    let (release, first) = mentioning.next()?;
    if !matches!(&first.kind, StatementKind::DecRef(p) if *p == whole) {
        return None;
    }
    let (_, next) = mentioning.next()?;
    let ends_value = match &next.kind {
        StatementKind::StorageDead(p, _) => *p == whole,
        StatementKind::Reassign(p, rvalue) => {
            *p == whole && !mentions(local, |mention| mention.visit_rvalue(rvalue, BasicBlock(0)))
        }
        StatementKind::Assign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => false,
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
    classifier: &RcClassifier,
) -> bool {
    match &local_decls[local.0].mir_ty {
        MirType::Custom(name) => classifier.runs_drop_hook(name),
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
