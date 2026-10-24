// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks every local a body records as borrowing ([`Body::borrowed_locals`])
//! against the body itself, after every reference-counting pass has run.
//!
//! A borrowed local holds no reference of its own: it reads a value out of a
//! place the body owns and relies on that place to keep the value alive. The
//! record is not trusted. Each entry is checked again from the MIR, by an
//! access classification of its own:
//!
//! - the local is one the body owns, holding a counted value with no drop hook
//!   and no device buffer, and every definition of it copies a projected place;
//! - that place is reached from a local the body owns — not a parameter, a
//!   capture or a local whose address is taken — through value steps only;
//! - the local is never rebound, released, written into a call's result,
//!   handed to a runtime entry point that keeps it or asks whether it is
//!   shared, passed `out`, launched over or merged in a phi; and unless its
//!   type is a reference by design, never written through nor handed to any
//!   callee but a runtime entry point that only reads it — a callee writes a
//!   parameter in place when nothing else holds it;
//! - on every path from a definition to the end of its storage, its owner —
//!   and, for a borrow read out of another borrowed local, that local's owners —
//!   is only read through a projection, retained through one, or lent by copy
//!   to a runtime entry point that only reads it;
//! - every owner it shows is recorded, since RC elision keeps the retains of
//!   exactly the recorded owners.
//!
//! What a runtime entry point does to its arguments is read from
//! `runtime_fns`, the ground truth both this check and the inference use. The
//! two type predicates — whether a step is a value step and whether a type is
//! a reference by design — are shared with the inference too
//! (`optimization::local_borrows`), so a defect in either passes both.
//!
//! An entry that passes is a borrow, and the ownership check leaves the local
//! untracked, as it does a borrowed parameter. One that fails is a violation.

use super::{local_display_name, VerificationViolation};
use crate::mir::optimization::local_borrows::{has_reference_semantics, reads_through_values};
use crate::mir::place::PlaceContext;
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::types::MirType;
use crate::mir::visitor::Visitor;
use crate::mir::{BasicBlock, Body, Local, Operand, Place, Rvalue, Statement, StatementKind};
use crate::runtime_fns::{
    is_copy_on_write, only_reads_argument, reads_reference_count, taken_argument_positions,
};
use std::collections::{HashMap, HashSet};

/// How a statement or terminator reaches a local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// Read through a projection, or its length taken.
    ProjectedRead,
    /// Copied whole.
    WholeRead,
    /// Moved out of, in an rvalue.
    Moved,
    /// Given a value whole by an assignment.
    Defined,
    /// Rebound whole, releasing what it held.
    Rebound,
    /// Written through a projection.
    WrittenThrough,
    /// Written by a call, as its destination.
    CallResult,
    /// Retained whole.
    Retained,
    /// Retained through a projection.
    ProjectedRetain,
    /// Released or deallocated.
    Released,
    /// Its storage started or ended.
    Storage,
    /// Its address taken.
    AddressTaken,
    /// Handed to a runtime entry point that only reads it
    /// (`runtime_fns::only_reads_argument`), or called as a closure.
    ReadByCallee,
    /// Handed to any other callee — a Miri function or method, a closure or a
    /// virtual call — by copy or by move, lent or owned. A callee writes a
    /// parameter in place when nothing else holds it.
    MayBeWritten,
    /// Handed to a call any other way, or to a kernel launch.
    Handed,
}

/// Every local `body` records as borrowing that the body bears out, and a
/// violation for each record it does not.
pub(super) fn held_borrows(body: &Body) -> (HashSet<Local>, Vec<VerificationViolation>) {
    let mut checker = Checker {
        body,
        decided: HashMap::new(),
    };
    let mut held = HashSet::new();
    let mut violations = Vec::new();
    let mut records: Vec<(&Local, &Vec<Local>)> = body.borrowed_locals.iter().collect();
    records.sort_unstable_by_key(|(local, _)| local.0);
    for (&local, recorded) in records {
        match checker.refusal(local, recorded) {
            None => {
                held.insert(local);
            }
            Some(reason) => violations.push(VerificationViolation {
                local,
                local_name: local_display_name(body, local),
                message: format!(
                    "recorded as borrowing from {} without its own reference, but {reason}",
                    owner_names(recorded)
                ),
            }),
        }
    }
    (held, violations)
}

/// Every record of a borrowed local in `body` the body does not bear out.
pub fn verify_local_borrows(body: &Body) -> Vec<VerificationViolation> {
    held_borrows(body).1
}

/// Checks records, remembering each local decided, so a borrow out of a
/// borrowed local reads that local's owners once.
struct Checker<'b> {
    body: &'b Body,
    /// `None` while a local is being decided, so a cycle of reads is refused.
    decided: HashMap<Local, Option<Option<Vec<Local>>>>,
}

impl Checker<'_> {
    /// Why the record that `local` borrows from the `recorded` owners does
    /// not hold, or `None` when it does. Every owner the body shows must be
    /// recorded: RC elision keeps the retains of exactly those.
    fn refusal(&mut self, local: Local, recorded: &[Local]) -> Option<String> {
        if let Some(reason) = self.local_refusal(local) {
            return Some(reason);
        }
        let Some(owners) = self.owners(local) else {
            return Some(
                "an owner of what it reads changes while it holds the value, or is not a \
                 local the body owns reached through value steps"
                    .to_string(),
            );
        };
        owners
            .iter()
            .find(|owner| !recorded.contains(owner))
            .map(|owner| format!("its owner {owner} is not recorded"))
    }

    /// Why `local` itself cannot borrow, whatever it reads.
    fn local_refusal(&self, local: Local) -> Option<String> {
        let body = self.body;
        let decl = &body.local_decls[local.0];
        if local.0 <= body.arg_count || body.env_capture_locals.contains(&local) {
            return Some("it is a parameter, a capture or the return slot".to_string());
        }
        if !body.is_counted_local(local) || decl.device_handle.is_some() {
            return Some("it holds no counted host value".to_string());
        }
        let has_drop_hook = matches!(&decl.mir_ty,
            MirType::Custom(name) | MirType::Vector(name) if body.has_drop_types.contains(name.as_str()));
        if has_drop_hook {
            return Some("its type has a drop hook".to_string());
        }
        if definitions_of(body, local).is_none() {
            return Some("a definition of it is not a copy of a projected place".to_string());
        }
        let reference = has_reference_semantics(&decl.ty.kind, body.rc_classifier.definitions());
        accesses_in(body)
            .find(|(accessed, access, _)| *accessed == local && refuses(*access, reference))
            .map(|(_, access, _)| format!("the body reaches it as {access:?}"))
    }

    /// The owners whose value `local` holds while it borrows, when every one
    /// of them stays unchanged over its range; `None` when it does not borrow.
    fn owners(&mut self, local: Local) -> Option<Vec<Local>> {
        if let Some(decided) = self.decided.get(&local) {
            return decided.clone().flatten();
        }
        self.decided.insert(local, None);
        let answer = self.settle(local);
        self.decided.insert(local, Some(answer.clone()));
        answer
    }

    fn settle(&mut self, local: Local) -> Option<Vec<Local>> {
        let body = self.body;
        let mut all_owners = Vec::new();
        for (site, source) in definitions_of(body, local)? {
            if !is_owned_root(body, source.local)
                || !reads_through_values(
                    &source,
                    |place| PlaceTypes::of(body).declared(place),
                    body.rc_classifier.definitions(),
                )
            {
                return None;
            }
            let mut owners = vec![source.local];
            if body.borrowed_locals.contains_key(&source.local) {
                owners.extend(self.owners(source.local)?);
            }
            if !stays_over_range(body, local, site, &owners) {
                return None;
            }
            all_owners.extend(owners);
        }
        Some(all_owners)
    }
}

/// The locals `owners` names, as a finding spells them.
fn owner_names(owners: &[Local]) -> String {
    owners
        .iter()
        .map(Local::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `access` to a candidate borrow refuses it; `reference` says
/// whether the value is a reference by design.
fn refuses(access: Access, reference: bool) -> bool {
    match access {
        Access::Rebound
        | Access::CallResult
        | Access::Released
        | Access::AddressTaken
        | Access::Handed => true,
        Access::WrittenThrough | Access::MayBeWritten => !reference,
        Access::ProjectedRead
        | Access::WholeRead
        | Access::Moved
        | Access::Defined
        | Access::Retained
        | Access::ProjectedRetain
        | Access::Storage
        | Access::ReadByCallee => false,
    }
}

/// Whether `access` to an owner leaves the value it holds where it was.
fn leaves_owner(access: Access) -> bool {
    match access {
        Access::ProjectedRead | Access::ProjectedRetain | Access::ReadByCallee => true,
        Access::WholeRead
        | Access::Moved
        | Access::Defined
        | Access::Rebound
        | Access::WrittenThrough
        | Access::CallResult
        | Access::Retained
        | Access::Released
        | Access::Storage
        | Access::AddressTaken
        | Access::MayBeWritten
        | Access::Handed => false,
    }
}

/// Each definition of `local` with the place it copies; `None` when one of
/// them is anything but a copy of a projected place.
fn definitions_of(body: &Body, local: Local) -> Option<Vec<((usize, usize), Place)>> {
    let mut reads = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for (index, statement) in data.statements.iter().enumerate() {
            let StatementKind::Assign(dest, rvalue) = &statement.kind else {
                continue;
            };
            if *dest != Place::new(local) {
                continue;
            }
            let Rvalue::Use(Operand::Copy(source)) = rvalue else {
                return None;
            };
            if source.projection.is_empty() {
                return None;
            }
            reads.push(((block, index), source.clone()));
        }
    }
    (!reads.is_empty()).then_some(reads)
}

/// Whether `root` is a local the body owns outright, whose address it never
/// takes.
fn is_owned_root(body: &Body, root: Local) -> bool {
    root.0 > body.arg_count
        && !body.env_capture_locals.contains(&root)
        && body.is_counted_local(root)
        && body.local_decls[root.0].device_handle.is_none()
        && !accesses_in(body)
            .any(|(local, access, _)| local == root && access == Access::AddressTaken)
}

/// Whether, on every path from the definition of `local` at `site` to the end
/// of its storage, each of `owners` is reached only in ways that leave it as
/// it was. A path reaching the body's end first does not end the range.
fn stays_over_range(body: &Body, local: Local, site: (usize, usize), owners: &[Local]) -> bool {
    let changes = |accesses: Vec<(Local, Access)>| {
        accesses
            .into_iter()
            .any(|(accessed, access)| owners.contains(&accessed) && !leaves_owner(access))
    };
    let mut pending = vec![(site.0, site.1 + 1)];
    let mut visited = HashSet::new();
    'blocks: while let Some((block, from)) = pending.pop() {
        let data = &body.basic_blocks[block];
        for statement in data.statements.iter().skip(from) {
            let ends = matches!(&statement.kind,
                StatementKind::StorageDead(p) | StatementKind::Assign(p, _) if *p == Place::new(local));
            if ends {
                continue 'blocks;
            }
            if changes(statement_accesses(statement)) {
                return false;
            }
        }
        let Some(terminator) = &data.terminator else {
            return false;
        };
        if matches!(terminator.kind, TerminatorKind::Return)
            || changes(terminator_accesses(terminator))
        {
            return false;
        }
        pending.extend(
            terminator
                .successors()
                .into_iter()
                .filter(|successor| visited.insert(successor.0))
                .map(|successor| (successor.0, 0)),
        );
    }
    true
}

/// Every access in `body`, with the block it is in.
fn accesses_in(body: &Body) -> impl Iterator<Item = (Local, Access, usize)> + '_ {
    body.basic_blocks
        .iter()
        .enumerate()
        .flat_map(|(block, data)| {
            let statements = data.statements.iter().flat_map(statement_accesses);
            let terminator = data.terminator.iter().flat_map(terminator_accesses);
            statements
                .chain(terminator)
                .map(move |(local, access)| (local, access, block))
        })
}

/// How `statement` reaches each local it names.
fn statement_accesses(statement: &Statement) -> Vec<(Local, Access)> {
    let mut accesses = Vec::new();
    match &statement.kind {
        StatementKind::Assign(dest, rvalue) => {
            push_place(&mut accesses, dest, Access::Defined, Access::WrittenThrough);
            rvalue_accesses(rvalue, &mut accesses);
        }
        StatementKind::Reassign(dest, rvalue) => {
            push_place(&mut accesses, dest, Access::Rebound, Access::WrittenThrough);
            rvalue_accesses(rvalue, &mut accesses);
        }
        StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
            push_place(&mut accesses, place, Access::Storage, Access::Storage);
        }
        StatementKind::IncRef(place) => {
            push_place(
                &mut accesses,
                place,
                Access::Retained,
                Access::ProjectedRetain,
            );
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
            push_place(&mut accesses, place, Access::Released, Access::Released);
        }
        StatementKind::Nop => {}
    }
    accesses
}

/// Records `place`'s root as reached `whole` when it has no projection and
/// `projected` otherwise, and each local indexing it as read.
fn push_place(
    accesses: &mut Vec<(Local, Access)>,
    place: &Place,
    whole: Access,
    projected: Access,
) {
    let access = if place.projection.is_empty() {
        whole
    } else {
        projected
    };
    accesses.push((place.local, access));
    accesses.extend(
        place
            .index_locals()
            .map(|index| (index, Access::ProjectedRead)),
    );
}

fn operand_accesses(operand: &Operand, accesses: &mut Vec<(Local, Access)>) {
    match operand {
        Operand::Copy(place) => {
            push_place(accesses, place, Access::WholeRead, Access::ProjectedRead)
        }
        Operand::Move(place) => push_place(accesses, place, Access::Moved, Access::Moved),
        Operand::Constant(_) | Operand::Function(_) => {}
    }
}

fn rvalue_accesses(rvalue: &Rvalue, accesses: &mut Vec<(Local, Access)>) {
    match rvalue {
        Rvalue::Use(operand) => operand_accesses(operand, accesses),
        Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => {
            operand_accesses(operand, accesses)
        }
        Rvalue::BinaryOp(_, left, right) => {
            operand_accesses(left, accesses);
            operand_accesses(right, accesses);
        }
        Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) => {
            operands
                .iter()
                .for_each(|operand| operand_accesses(operand, accesses));
        }
        // Perceus funds no operand of a phi.
        Rvalue::Phi(arms) => accesses.extend(
            arms.iter()
                .filter_map(|(operand, _)| operand.place())
                .map(|place| (place.local, Access::Handed)),
        ),
        Rvalue::Ref(place) => {
            push_place(accesses, place, Access::AddressTaken, Access::AddressTaken)
        }
        Rvalue::Len(place) => push_place(
            accesses,
            place,
            Access::ProjectedRead,
            Access::ProjectedRead,
        ),
        Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => {
            let mut mentioned = Mentioned::default();
            mentioned.visit_rvalue(rvalue, BasicBlock(0));
            accesses.extend(
                mentioned
                    .locals
                    .into_iter()
                    .map(|local| (local, Access::Handed)),
            );
        }
    }
}

/// How `terminator` reaches each local it names.
fn terminator_accesses(terminator: &Terminator) -> Vec<(Local, Access)> {
    let mut accesses = Vec::new();
    let (func, args, arg_modes, destination) = match &terminator.kind {
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            destination,
            ..
        } => (Some(func), args, arg_modes, destination),
        TerminatorKind::VirtualCall {
            args,
            arg_modes,
            destination,
            ..
        } => (None, args, arg_modes, destination),
        TerminatorKind::SwitchInt { discr, .. } => {
            operand_accesses(discr, &mut accesses);
            return accesses;
        }
        TerminatorKind::GpuLaunch { .. } => {
            let mut mentioned = Mentioned::default();
            mentioned.visit_terminator(BasicBlock(0), terminator);
            return mentioned
                .locals
                .into_iter()
                .map(|local| (local, Access::Handed))
                .collect();
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {
            return accesses;
        }
    };
    push_place(
        &mut accesses,
        destination,
        Access::CallResult,
        Access::CallResult,
    );
    match func {
        Some(Operand::Copy(place)) => push_place(
            &mut accesses,
            place,
            Access::ReadByCallee,
            Access::ReadByCallee,
        ),
        Some(Operand::Move(place)) => {
            push_place(&mut accesses, place, Access::Handed, Access::Handed)
        }
        Some(Operand::Constant(_) | Operand::Function(_)) | None => {}
    }
    let runtime = func.and_then(Operand::called_runtime_name);
    for (position, arg) in args.iter().enumerate() {
        if let Some(place) = arg.place() {
            let access = argument_access(runtime, position, arg_mode(arg_modes, position));
            push_place(&mut accesses, place, access, access);
        }
    }
    accesses
}

/// How a call reaches its argument at `position`, passed `mode`, when its
/// callee is the runtime entry point `runtime` (`None` for any other).
fn argument_access(runtime: Option<&str>, position: usize, mode: ArgMode) -> Access {
    let checked_or_kept = runtime.is_some_and(|name| {
        is_copy_on_write(name)
            || reads_reference_count(name)
            || taken_argument_positions(name).contains(&position)
    });
    // A move handed to such an entry point is a uniqueness witness at most;
    // the entry point still only reads it.
    let only_read = runtime.is_some_and(|name| only_reads_argument(name, position));
    match mode {
        ArgMode::Borrowed | ArgMode::Owned | ArgMode::Out if checked_or_kept => Access::Handed,
        ArgMode::Borrowed if only_read => Access::ReadByCallee,
        ArgMode::Borrowed | ArgMode::Owned => Access::MayBeWritten,
        ArgMode::Out => Access::Handed,
    }
}

/// Collects every local a MIR fragment names.
#[derive(Default)]
struct Mentioned {
    locals: Vec<Local>,
}

impl Visitor for Mentioned {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.locals.push(local);
    }
}
