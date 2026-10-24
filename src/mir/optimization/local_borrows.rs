// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Borrowed locals: a managed value read out of a place the body owns takes
//! no reference while nothing can change or release that place.
//!
//! A local `l = copy p`, where `p` is reached from a root `r` through value
//! steps only — an element of a list or an array, a field of a struct, a tuple
//! or an optional, an enum's payload — reads a value `r` already holds. While
//! `r` stays as it was, `r`'s reference keeps that value alive, so `l` can
//! borrow it the way a body borrows a parameter: no retain when it is read, no
//! release when its storage ends. Perceus funds every way a borrowed value
//! escapes: a copy into a store, a return, an aggregate or a capture is
//! retained, a move out of it is retained, and an owned argument is funded. So
//! what has to be proved is only that the owner outlives the borrow.
//!
//! `l` borrows when all of these hold:
//!
//! - **It reads.** `l` is a managed local the body owns, every definition of it
//!   is a copy Perceus would retain, it runs no drop hook and holds no device
//!   buffer, and it is never rebound, released by hand, handed to a runtime
//!   entry point that keeps it, written through, or passed `out`.
//! - **It is never asked whether it is shared**, unless its type is a reference
//!   by design — a class, or a trait only classes implement. A borrowed value's
//!   count is its owner's, so a copy-on-write check on it would answer "not
//!   shared" and write into the owner's element. Every write through `l`, every
//!   uniqueness check on it and every move of it into a call is refused.
//! - **Its root is a local the body owns** — not a parameter and not a capture —
//!   and every step from the root to `p` is a value step. Nothing crosses a
//!   class field: a method can clear a field it never names here. Any other
//!   holder of a value the body owns holds its own reference and copies before
//!   writing, so only this body can change what `r` holds.
//! - **Its root is not touched while it borrows.** On every path from a
//!   definition of `l` to the end of `l`'s storage, the root is only read
//!   through a projection, or lent to a call by copy: never written, moved,
//!   copied whole, passed owned or `out`, released, or ended. A borrow read out
//!   of another borrowed local inherits that local's root, so its root's root
//!   must stay as well, and the end of its root's storage is a change. A path
//!   that reaches the end of the body before `l`'s storage ends is refused.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, STRING_TYPE_NAME};
use crate::mir::block::BasicBlockData;
use crate::mir::optimization::perceus::{place_declared_type, Perceus, PerceusContext};
use crate::mir::place::PlaceContext;
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::types::MirType;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
};
use crate::runtime_fns::{
    cow_fn, is_copy_on_write, only_reads_argument, reads_reference_count, taken_argument_positions,
};
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

/// A statement's position: its block and its index in the block.
type Site = (usize, usize);

/// Every read a candidate local is defined by, with the place it reads.
type Reads = HashMap<Local, Vec<(Site, Place)>>;

/// The locals of `body` that borrow, each with every local whose value keeps
/// what it holds alive; see the module documentation.
pub(super) fn infer_local_borrows(
    body: &Body,
    ctx: &PerceusContext,
    managed_locals: &HashSet<Local>,
) -> HashMap<Local, Vec<Local>> {
    let mut reads = borrowing_reads(body, ctx, managed_locals);
    reads.retain(|&local, _| is_plain_value(body, local));
    let refused = refused_locals(body, ctx, &reads);
    reads.retain(|local, _| !refused.contains(local));
    let roots: HashSet<Local> = reads
        .values()
        .flatten()
        .map(|(_, source)| source.local)
        .collect();
    let address_taken = address_taken_locals(body, &roots);
    reads.retain(|&local, sources| {
        sources.iter().all(|(_, source)| {
            source.local != local
                && is_owned_root(body, source.local)
                && managed_locals.contains(&source.local)
                && !address_taken.contains(&source.local)
                && reads_through_values(source, |p| place_declared_type(p, ctx), definitions(ctx))
        })
    });
    let mut settled = Settled {
        body,
        reads: &reads,
        decided: HashMap::new(),
        owners: HashMap::new(),
    };
    for &local in reads.keys() {
        settled.borrows(local);
    }
    settled.owners
}

/// Decides, parents first, which candidates borrow: a candidate read out of
/// another candidate depends on whether that one borrows, since a borrowed
/// parent's own root must then stay as well.
struct Settled<'b> {
    body: &'b Body,
    reads: &'b Reads,
    /// Each candidate decided so far; `None` while it is being decided, so a
    /// candidate read, around a loop, out of itself is refused.
    decided: HashMap<Local, Option<bool>>,
    /// Each candidate that borrows, with the owners of what it holds.
    owners: HashMap<Local, Vec<Local>>,
}

impl Settled<'_> {
    /// Whether `local`, a candidate, borrows; `None` when deciding it needs
    /// a candidate still being decided — a cycle of reads — which refuses
    /// every candidate on it.
    fn borrows(&mut self, local: Local) -> Option<bool> {
        if let Some(decided) = self.decided.get(&local) {
            return *decided;
        }
        self.decided.insert(local, None);
        let reads = self.reads;
        let mut answer = Some(true);
        let mut all_owners: Vec<Local> = Vec::new();
        for (site, source) in reads.get(&local).into_iter().flatten() {
            let Some(owners) = self.owners_of(source.local) else {
                answer = None;
                continue;
            };
            if !owner_stays(self.body, local, *site, &owners) {
                answer = answer.and(Some(false));
            }
            for owner in owners {
                if !all_owners.contains(&owner) {
                    all_owners.push(owner);
                }
            }
        }
        if answer == Some(true) {
            self.owners.insert(local, all_owners);
        }
        self.decided.insert(local, Some(answer == Some(true)));
        answer
    }

    /// `root` and, while it is itself a borrowed read, the owners of what it
    /// reads: every local whose change could release what a borrow out of
    /// `root` holds. `None` on a cycle of reads.
    fn owners_of(&mut self, root: Local) -> Option<Vec<Local>> {
        let mut owners = vec![root];
        if !self.reads.contains_key(&root) || !self.borrows(root)? {
            return Some(owners);
        }
        let reads = self.reads;
        for (_, source) in reads.get(&root).into_iter().flatten() {
            for owner in self.owners_of(source.local)? {
                if !owners.contains(&owner) {
                    owners.push(owner);
                }
            }
        }
        Some(owners)
    }
}

fn definitions<'a>(ctx: &PerceusContext<'a>) -> &'a HashMap<String, TypeDefinition> {
    ctx.rc_classifier.definitions()
}

/// Each managed local's copies of a projected place that Perceus would
/// retain, by the local they define.
fn borrowing_reads(body: &Body, ctx: &PerceusContext, managed_locals: &HashSet<Local>) -> Reads {
    let mut reads = Reads::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for (index, statement) in data.statements.iter().enumerate() {
            let StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(source))) = &statement.kind
            else {
                continue;
            };
            let is_read = dest.projection.is_empty()
                && managed_locals.contains(&dest.local)
                && !source.projection.is_empty()
                && Perceus.should_incref_source(ctx, source, dest, managed_locals);
            if is_read {
                reads
                    .entry(dest.local)
                    .or_default()
                    .push(((block, index), source.clone()));
            }
        }
    }
    reads
}

/// Whether `local`'s release could only ever free memory: it runs no drop
/// hook and holds no device buffer.
fn is_plain_value(body: &Body, local: Local) -> bool {
    let decl = &body.local_decls[local.0];
    let has_drop_hook = matches!(&decl.mir_ty,
        MirType::Custom(name) | MirType::Vector(name) if body.has_drop_types.contains(name.as_str()));
    !has_drop_hook && decl.device_handle.is_none()
}

/// Whether `root` is a local the body owns outright: not the return slot, a
/// parameter or a capture, and holding no device buffer.
fn is_owned_root(body: &Body, root: Local) -> bool {
    root.0 > body.arg_count
        && !body.env_capture_locals.contains(&root)
        && body.local_decls[root.0].device_handle.is_none()
}

/// The candidates something in `body` uses in a way a borrow cannot carry:
/// a definition that is not a read, a rebinding, a release by hand, a write
/// through it, a runtime entry point keeping or checking it, an `out`
/// argument, a kernel launch, or — for a value that is not a reference by
/// design — a move into a call.
fn refused_locals(body: &Body, ctx: &PerceusContext, reads: &Reads) -> HashSet<Local> {
    let reads_at: HashSet<(Local, Site)> = reads
        .iter()
        .flat_map(|(&local, sources)| sources.iter().map(move |(site, _)| (local, *site)))
        .collect();
    let is_reference =
        |local: Local| has_reference_semantics(&ctx.local_decls[local.0].ty.kind, definitions(ctx));
    let mut refused = HashSet::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for (index, statement) in data.statements.iter().enumerate() {
            let is_read = |local: Local| reads_at.contains(&(local, (block, index)));
            refused.extend(
                refused_by_statement(statement, is_read)
                    .into_iter()
                    .filter(|local| reads.contains_key(local)),
            );
        }
        if let Some(terminator) = &data.terminator {
            refused.extend(
                refused_by_terminator(terminator, is_reference)
                    .into_iter()
                    .filter(|local| reads.contains_key(local)),
            );
        }
    }
    refused.extend(written_through_values(body, reads, is_reference));
    refused
}

/// The locals `statement` defines other than by a recorded read, rebinds,
/// releases by hand, takes the address of or hands to a GPU operation.
fn refused_by_statement(statement: &Statement, is_read: impl Fn(Local) -> bool) -> Vec<Local> {
    match &statement.kind {
        StatementKind::Assign(dest, rvalue) => {
            let mut refused = refused_by_rvalue(rvalue);
            if dest.projection.is_empty() && !is_read(dest.local) {
                refused.push(dest.local);
            }
            refused
        }
        StatementKind::Reassign(dest, rvalue) => {
            let mut refused = refused_by_rvalue(rvalue);
            if dest.projection.is_empty() {
                refused.push(dest.local);
            }
            refused
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => vec![place.local],
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::Nop => Vec::new(),
    }
}

/// The locals `rvalue` takes the address of, hands to a GPU operation, or
/// merges in a phi.
fn refused_by_rvalue(rvalue: &Rvalue) -> Vec<Local> {
    match rvalue {
        Rvalue::Ref(place) => vec![place.local],
        Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => {
            let mut mentions = Mentions::default();
            mentions.visit_rvalue(rvalue, BasicBlock(0));
            mentions.locals
        }
        // Perceus funds no operand of a phi, so a borrow must not flow
        // through one.
        Rvalue::Phi(arms) => arms
            .iter()
            .filter_map(|(operand, _)| operand.place())
            .map(|place| place.local)
            .collect(),
        Rvalue::Use(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::Aggregate(..) => Vec::new(),
    }
}

/// The locals a terminator writes, launches, calls by moving, passes `out`,
/// hands to a runtime entry point that keeps or checks them, or — unless
/// `is_reference` — hands to any callee not known to only read it. A callee
/// writes a parameter in place when nothing else holds it, and a borrowed
/// value's count is its owner's.
fn refused_by_terminator(
    terminator: &Terminator,
    is_reference: impl Fn(Local) -> bool,
) -> Vec<Local> {
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
        TerminatorKind::GpuLaunch { .. } => return mentioned_locals(terminator),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => return Vec::new(),
    };
    let refused_args = args.iter().enumerate().filter_map(|(position, arg)| {
        let place = arg.place()?;
        let keeps =
            keeps_or_checks(func, position) || arg_mode(arg_modes, position) == ArgMode::Out;
        let may_write_a_value = !is_reference(place.local) && !only_reads(func, position);
        (keeps || may_write_a_value).then_some(place.local)
    });
    let moved_callee = match func {
        Some(Operand::Move(place)) => Some(place.local),
        Some(Operand::Copy(_) | Operand::Constant(_) | Operand::Function(_)) | None => None,
    };
    std::iter::once(destination.local)
        .chain(moved_callee)
        .chain(refused_args)
        .collect()
}

/// Whether `func` is a runtime entry point that only reads the argument at
/// `position` ([`only_reads_argument`]). Any other callee, a Miri function or
/// method, a closure or a virtual call, may write it.
fn only_reads(func: Option<&Operand>, position: usize) -> bool {
    func.and_then(Operand::called_runtime_name)
        .is_some_and(|name| only_reads_argument(name, position))
}

/// Whether the call to `func` keeps the argument at `position`, or asks
/// whether it is shared.
fn keeps_or_checks(func: Option<&Operand>, position: usize) -> bool {
    func.and_then(Operand::called_runtime_name)
        .is_some_and(|name| {
            is_copy_on_write(name)
                || reads_reference_count(name)
                || taken_argument_positions(name).contains(&position)
        })
}

/// The candidates that are not references by design and are written through
/// a projection: such a write follows a uniqueness check on the candidate.
fn written_through_values(
    body: &Body,
    reads: &Reads,
    is_reference: impl Fn(Local) -> bool,
) -> Vec<Local> {
    body.basic_blocks
        .iter()
        .flat_map(|data| &data.statements)
        .filter_map(Statement::written_place)
        .filter(|place| !place.projection.is_empty() && reads.contains_key(&place.local))
        .map(|place| place.local)
        .filter(|&local| !is_reference(local))
        .collect()
}

/// The `roots` whose address `body` takes anywhere.
fn address_taken_locals(body: &Body, roots: &HashSet<Local>) -> HashSet<Local> {
    body.basic_blocks
        .iter()
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
        .filter(|local| roots.contains(local))
        .collect()
}

/// What a block holds of a borrow's range, from a statement on.
enum Segment {
    /// The range ends in the block: the borrow's storage ends or it is
    /// defined again.
    Ended,
    /// Something in the block changes an owner.
    Changed,
    /// Nothing ends the range or changes an owner; it carries on into the
    /// block's successors.
    Continues(Vec<BasicBlock>),
}

/// Whether nothing in `owners` changes on any path from the definition of
/// `local` at `site` to the end of its storage.
fn owner_stays(body: &Body, local: Local, site: Site, owners: &[Local]) -> bool {
    let mut pending = vec![(site.0, site.1 + 1)];
    let mut entered = HashSet::new();
    while let Some((block, from)) = pending.pop() {
        match segment(&body.basic_blocks[block], local, from, owners) {
            Segment::Changed => return false,
            Segment::Ended => {}
            Segment::Continues(successors) => pending.extend(
                successors
                    .into_iter()
                    .filter(|successor| entered.insert(successor.0))
                    .map(|successor| (successor.0, 0)),
            ),
        }
    }
    true
}

/// How `data`, from statement `from` on, extends the range of `local`.
fn segment(data: &BasicBlockData, local: Local, from: usize, owners: &[Local]) -> Segment {
    for statement in data.statements.iter().skip(from) {
        if ends_range(statement, local) {
            return Segment::Ended;
        }
        if statement_changes(statement, owners) {
            return Segment::Changed;
        }
    }
    match data.terminator.as_ref() {
        Some(terminator) if !terminator_changes(terminator, owners) => match &terminator.kind {
            TerminatorKind::Return => Segment::Changed,
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Unreachable
            | TerminatorKind::Call { .. }
            | TerminatorKind::VirtualCall { .. }
            | TerminatorKind::GpuLaunch { .. } => Segment::Continues(terminator.successors()),
        },
        Some(_) | None => Segment::Changed,
    }
}

/// Whether `statement` ends the storage of `local` or defines it again.
fn ends_range(statement: &Statement, local: Local) -> bool {
    match &statement.kind {
        StatementKind::StorageDead(place) | StatementKind::Assign(place, _) => {
            *place == Place::new(local)
        }
        StatementKind::Reassign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => false,
    }
}

/// Whether `statement` changes one of `owners`: writes, ends, releases or
/// retains it whole, or reads it other than through a projection.
fn statement_changes(statement: &Statement, owners: &[Local]) -> bool {
    match &statement.kind {
        StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
            owners.contains(&dest.local) || rvalue_changes(rvalue, owners)
        }
        StatementKind::StorageLive(place)
        | StatementKind::StorageDead(place)
        | StatementKind::DecRef(place)
        | StatementKind::Dealloc(place) => owners.contains(&place.local),
        StatementKind::IncRef(place) => {
            owners.contains(&place.local) && place.projection.is_empty()
        }
        StatementKind::Nop => false,
    }
}

/// Whether `rvalue` moves, copies whole or takes the address of one of
/// `owners`. A read through a projection and a length leave it as it was.
fn rvalue_changes(rvalue: &Rvalue, owners: &[Local]) -> bool {
    let operand_changes = |operand: &Operand| operand_changes(operand, owners);
    match rvalue {
        Rvalue::Use(operand) => operand_changes(operand),
        Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => operand_changes(operand),
        Rvalue::BinaryOp(_, left, right) => operand_changes(left) || operand_changes(right),
        Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) => {
            operands.iter().any(operand_changes)
        }
        Rvalue::Phi(arms) => arms.iter().any(|(operand, _)| operand_changes(operand)),
        Rvalue::Ref(place) => owners.contains(&place.local),
        Rvalue::Len(_) => false,
        Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => {
            let mut mentions = Mentions::default();
            mentions.visit_rvalue(rvalue, BasicBlock(0));
            mentions.locals.iter().any(|local| owners.contains(local))
        }
    }
}

/// Whether `operand` moves one of `owners`, or copies one whole.
fn operand_changes(operand: &Operand, owners: &[Local]) -> bool {
    match operand {
        Operand::Copy(place) => owners.contains(&place.local) && place.projection.is_empty(),
        Operand::Move(place) => owners.contains(&place.local),
        Operand::Constant(_) | Operand::Function(_) => false,
    }
}

/// Whether `terminator` changes one of `owners`: writes it, launches a kernel
/// over it, or hands it to a call other than lent by copy to a runtime entry
/// point that only reads it. Any other callee may write what it is lent in
/// place.
fn terminator_changes(terminator: &Terminator, owners: &[Local]) -> bool {
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
        TerminatorKind::SwitchInt { discr, .. } => return operand_changes(discr, owners),
        TerminatorKind::GpuLaunch { .. } => {
            return mentioned_locals(terminator)
                .iter()
                .any(|local| owners.contains(local))
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {
            return false
        }
    };
    let names_owner =
        |operand: &Operand| operand.place().is_some_and(|p| owners.contains(&p.local));
    let lends = |position: usize, arg: &Operand| {
        matches!(arg, Operand::Copy(_))
            && arg_mode(arg_modes, position) == ArgMode::Borrowed
            && only_reads(func, position)
    };
    owners.contains(&destination.local)
        || func.is_some_and(names_owner)
        || args
            .iter()
            .enumerate()
            .any(|(position, arg)| names_owner(arg) && !lends(position, arg))
}

/// Every local `terminator` names.
fn mentioned_locals(terminator: &Terminator) -> Vec<Local> {
    let mut mentions = Mentions::default();
    mentions.visit_terminator(BasicBlock(0), terminator);
    mentions.locals
}

/// Collects every local a MIR fragment names.
#[derive(Default)]
struct Mentions {
    locals: Vec<Local>,
}

impl Visitor for Mentions {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.locals.push(local);
    }
}

/// Whether every step from `place`'s root to it is a value step: an element
/// of a list or an array, or a field of a struct, a tuple, an optional or an
/// enum. `declared` gives the declared type of each place on the way.
pub(crate) fn reads_through_values(
    place: &Place,
    declared: impl Fn(&Place) -> Option<Type>,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    !place.projection.is_empty()
        && place.projection.iter().enumerate().all(|(depth, step)| {
            let container = Place {
                local: place.local,
                projection: place.projection[..depth].to_vec(),
            };
            declared(&container).is_some_and(|ty| is_value_step(&ty.kind, step, definitions))
        })
}

/// Whether `step` out of a value of `container` reaches a part of that value
/// rather than something it refers to.
fn is_value_step(
    container: &TypeKind,
    step: &PlaceElem,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    match step {
        PlaceElem::Index(_) => matches!(
            container.as_builtin_collection(),
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        ),
        PlaceElem::Field(_) => holds_fields_by_value(container, definitions),
        PlaceElem::Deref => false,
    }
}

/// Whether a field of a value of `kind` is part of that value: an optional's
/// payload, a tuple's element, a struct's field or an enum's payload.
fn holds_fields_by_value(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> bool {
    match kind {
        TypeKind::Option(_) | TypeKind::Tuple(_) => true,
        TypeKind::Custom(name, _) => {
            kind.as_builtin_collection().is_none()
                && match definitions.get(name) {
                    Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => true,
                    Some(
                        TypeDefinition::Class(_)
                        | TypeDefinition::Trait(_)
                        | TypeDefinition::Generic(_)
                        | TypeDefinition::Alias(_),
                    )
                    | None => false,
                }
        }
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::OneOf(_) => false,
    }
}

/// Whether a value of `kind` is a reference by design, never copied on
/// write: an instance of a class, or a value of a trait only such classes
/// implement. A built-in collection and a string are classes that copy on
/// write, and so are not.
pub(crate) fn has_reference_semantics(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    let TypeKind::Custom(name, _) = kind else {
        return false;
    };
    match definitions.get(name) {
        Some(TypeDefinition::Class(_)) => is_reference_class(name, definitions),
        Some(TypeDefinition::Trait(_)) => implementers_are_reference_classes(name, definitions),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// The class `name` and every class it extends, nearest first.
fn class_chain<'a>(
    name: &'a str,
    definitions: &'a HashMap<String, TypeDefinition>,
) -> impl Iterator<Item = &'a str> {
    let mut seen = HashSet::new();
    std::iter::successors(Some(name), move |current| match definitions.get(*current) {
        Some(TypeDefinition::Class(class)) => class.base_class.as_deref(),
        Some(_) | None => None,
    })
    .take_while(move |current| seen.insert(*current))
}

/// Whether neither the class `name` nor any class it extends copies on write.
fn is_reference_class(name: &str, definitions: &HashMap<String, TypeDefinition>) -> bool {
    class_chain(name, definitions).all(|class| {
        let copies_on_write = BuiltinCollectionKind::from_name(class)
            .and_then(cow_fn)
            .is_some();
        // A string appends in place when nothing else holds it
        // (`taken_argument_positions`), so it is a value too.
        !copies_on_write && class != STRING_TYPE_NAME
    })
}

/// Whether the trait `name`, or a trait extending it, is implemented, and
/// only by reference classes. A trait no class implements has no value to
/// judge by, and is not taken as a reference.
fn implementers_are_reference_classes(
    name: &str,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    definitions
        .iter()
        .filter_map(|(class_name, definition)| match definition {
            TypeDefinition::Class(class) => Some((class_name, class)),
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Trait(_) => None,
        })
        .filter(|(class_name, _)| {
            class_chain(class_name, definitions).any(|ancestor| {
                matches!(definitions.get(ancestor), Some(TypeDefinition::Class(class))
                    if class.traits.iter().any(|t| trait_extends(t, name, definitions)))
            })
        })
        .try_fold(false, |_, (class_name, _)| {
            is_reference_class(class_name, definitions).then_some(true)
        })
        .unwrap_or(false)
}

/// Whether the trait `name` is `target` or extends it.
fn trait_extends(name: &str, target: &str, definitions: &HashMap<String, TypeDefinition>) -> bool {
    let mut pending = vec![name];
    let mut seen = HashSet::new();
    while let Some(current) = pending.pop() {
        if current == target {
            return true;
        }
        if !seen.insert(current) {
            continue;
        }
        if let Some(TypeDefinition::Trait(definition)) = definitions.get(current) {
            pending.extend(definition.parent_traits.iter().map(String::as_str));
        }
    }
    false
}
