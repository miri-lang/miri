// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks every local a body records as borrowing ([`OwnershipFacts::borrowed_locals`])
//! against the body itself, after every reference-counting pass has run.
//!
//! A borrowed local holds no reference of its own: it reads a value out of a
//! place the body owns and relies on that place to keep the value alive. The
//! record is not trusted. Each entry is checked again from the MIR, by an
//! access classification of its own:
//!
//! - the local is one the body owns, holding a counted value and no device
//!   buffer, and every definition of it copies a projected place; a value
//!   whose release runs a drop hook borrows like any other, its owner running
//!   the hook;
//! - that place is reached from a counted local the body owns, or a counted
//!   parameter other than an `out` one — never a capture or a local whose
//!   address is taken — through value steps and class fields only;
//! - the local is never rebound, released, written into a call's result,
//!   handed to a runtime entry point that keeps it or asks whether it is
//!   shared, passed `out`, handed by `move` to a position its callee owns,
//!   or launched over; and unless its
//!   type is a reference by design, never written through nor handed to any
//!   callee but a runtime entry point that only reads it or a function of the
//!   program its caller found harmless ([`super::callee_effects`]), which
//!   writes no parameter, its receiver included — except that a value
//!   a write copies (`type_facts::unsharing`) may be lent whole, by copy, to a
//!   function of the program or to a method past its receiver, since a body
//!   writing such a parameter binds a copy of it on entry. A method's receiver
//!   gets no copy: a write through it relies on the caller's copy-on-write, or
//!   checks that it arrived unique (`RECEIVER_UNIQUE`), and a borrowed value's
//!   count is its owner's;
//! - on every path from a definition to the end of its storage, its owner —
//!   and, for a borrow read out of another borrowed local, that local's owners —
//!   is only read through a projection, retained through one, or lent by copy
//!   to a runtime entry point that only reads it or to a function of the
//!   program its caller found harmless ([`super::callee_effects`]), which
//!   stores into nothing but its own locals; the end of a lent parameter's
//!   binding leaves its caller's reference in place; a store into or release
//!   of an owner that parts from every place copied out of it on the way at a
//!   field of a tuple, a struct or a class instance ([`parts_at_own_field`])
//!   leaves the value where it was, and is harmful only through a drop hook
//!   the value it releases may run;
//! - an entry a runtime getter lends out of a collection
//!   ([`RuntimeEffect::hands_back_a_borrow`]) holds no reference of its own: a
//!   borrow read out of one counts the locals holding the collection among
//!   its owners, and other code may reach it;
//! - when what it reads is reached through a class field, out of a
//!   parameter or out of a lent entry — directly or through a borrowed
//!   parent — every operation on such a path is harmless by the verifiers'
//!   own rule ([`super::callee_effects`]), with the calls to the program its
//!   caller found harmless by that rule: never the pass's own answer,
//!   the `CallFlags::quiet` stamp on a call, and with none every such call is taken as harmful;
//! - every owner it shows is recorded, since RC elision keeps the retains of
//!   exactly the recorded owners.
//!
//! What a runtime entry point does to its arguments is read from the record
//! its call carries, the ground truth both this check and the inference use. The
//! type predicates — how a step reaches what it reads and whether a type is
//! a reference by design — are worked out again here
//! ([`super::borrow_reach`]), never asked of the inference, so a defect in
//! one is caught by the other.
//!
//! An entry that passes is a borrow, and the ownership check leaves the local
//! untracked, as it does a borrowed parameter. One that fails is a violation.

use super::borrow_reach::{is_reference_by_design, parts_at_own_field, reach_of, Reach};
use super::callee_effects::BodyHarm;
use super::{local_display_name, VerificationViolation};
use crate::ast::types::Type;
use crate::mir::facts::OwnershipFacts;
use crate::mir::place::PlaceContext;
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::type_facts::DropHookReach;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, Local, Operand, Place, Rvalue, Statement, StatementKind,
};
use crate::runtime_effects::RuntimeEffect;
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
    /// ([`RuntimeEffect::only_reads`]), or called as a closure.
    ReadByCallee,
    /// Lent whole, by copy, to a parameter of a function of the program or of
    /// a method past its receiver: a body writing such a parameter binds a
    /// copy of it on entry when its type is one a write copies.
    LentToEntryCopy,
    /// Handed to any other callee — a method as its receiver, a closure or a
    /// virtual call, or any callee by move or owned. A method writes its
    /// receiver in place, by the count its caller hands it
    /// (`RECEIVER_UNIQUE`).
    MayBeWritten,
    /// Copied whole into a container a runtime entry point stores it in,
    /// passed [`ArgMode::Donated`]: the call retains it first, so the
    /// container's reference is its own and the value is only read.
    Stored,
    /// Handed to a call any other way, or to a kernel launch.
    Handed,
}

/// Every local `body` records as borrowing that the body bears out, and a
/// violation for each record it does not. `harmless_calls` are the calls to
/// the program found harmless by [`super::callee_effects`]; `None` takes
/// every such call as harmful.
pub(super) fn held_borrows(
    body: &Body,
    ownership: &OwnershipFacts,
    harmless_calls: Option<&HashSet<BasicBlock>>,
) -> (HashSet<Local>, Vec<VerificationViolation>) {
    let mut held = HashSet::new();
    let mut violations = Vec::new();
    if ownership.borrowed_locals.is_empty() {
        return (held, violations);
    }
    let hooks = DropHookReach::new(body.rc_classifier.definitions());
    let mut checker = Checker {
        body,
        ownership,
        harm: BodyHarm::of(body, &hooks, harmless_calls),
        entries: entry_collections(body),
        decided: HashMap::new(),
    };
    let mut records: Vec<(&Local, &Vec<Local>)> = ownership.borrowed_locals.iter().collect();
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

/// Every record of a borrowed local in `body` the body does not bear out,
/// with `harmless_calls` as for [`held_borrows`].
pub fn verify_local_borrows(
    body: &Body,
    ownership: &OwnershipFacts,
    harmless_calls: Option<&HashSet<BasicBlock>>,
) -> Vec<VerificationViolation> {
    held_borrows(body, ownership, harmless_calls).1
}

/// The owners a borrow's value is held by, the places read out of them on
/// the way to it, and whether other code may reach it — through a class
/// field or out of a parameter.
#[derive(Clone)]
struct Owners {
    locals: Vec<Local>,
    /// Each place a definition on the way copies, rooted at one of `locals`.
    /// A local holding the collection an entry is lent from has none.
    read: Vec<Place>,
    is_exposed: bool,
}

/// Checks records, remembering each local decided, so a borrow out of a
/// borrowed local reads that local's owners once.
struct Checker<'b> {
    body: &'b Body,
    ownership: &'b OwnershipFacts,
    /// Which operations are harmless.
    harm: BodyHarm<'b>,
    /// Each local holding an entry a collection lends, with the locals
    /// holding that collection.
    entries: HashMap<Local, Vec<Local>>,
    /// `None` while a local is being decided, so a cycle of reads is refused.
    decided: HashMap<Local, Option<Option<Owners>>>,
}

impl Checker<'_> {
    /// Why the record that `local` borrows from the `recorded` owners does
    /// not hold, or `None` when it does. Every owner the body shows must be
    /// recorded: RC elision keeps the retains of exactly those.
    fn refusal(&mut self, local: Local, recorded: &[Local]) -> Option<String> {
        if let Some(reason) = self.local_refusal(local) {
            return Some(reason);
        }
        let Some(Owners { locals: owners, .. }) = self.owners(local) else {
            return Some(
                "an owner of what it reads changes while it holds the value, or is not a \
                 counted local or parameter reached through value steps and class fields"
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
        if local.0 <= body.arg_count || body.captures.env_capture_locals.contains(&local) {
            return Some("it is a parameter, a capture or the return slot".to_string());
        }
        if !body.is_counted_local(local) || decl.device_handle.is_some() {
            return Some("it holds no counted host value".to_string());
        }
        if definitions_of(body, local).is_none() {
            return Some(
                "a definition of it is not a copy of a projected place, or of a whole local into a temporary"
                    .to_string(),
            );
        }
        let writes = written_as(body, &decl.ty);
        credited_accesses_in(body, &self.harm)
            .find(|(accessed, access)| *accessed == local && refuses(*access, writes))
            .map(|(_, access)| format!("the body reaches it as {access:?}"))
    }

    /// The owners whose value `local` holds while it borrows, when every one
    /// of them stays unchanged over its range; `None` when it does not borrow.
    fn owners(&mut self, local: Local) -> Option<Owners> {
        if let Some(decided) = self.decided.get(&local) {
            return decided.clone().flatten();
        }
        self.decided.insert(local, None);
        let answer = self.settle(local);
        self.decided.insert(local, Some(answer.clone()));
        answer
    }

    fn settle(&mut self, local: Local) -> Option<Owners> {
        let body = self.body;
        let mut all = Owners {
            locals: Vec::new(),
            read: Vec::new(),
            is_exposed: false,
        };
        for (site, source) in definitions_of(body, local)? {
            if !is_root(body, source.local) {
                return None;
            }
            let reach = reach_of(body, &source)?;
            let mut owners = vec![source.local];
            let mut read = Vec::new();
            let mut read_exposed = reach == Reach::ThroughField || is_parameter(body, source.local);
            if let Some(collections) = self.entries.get(&source.local) {
                owners.extend(collections);
                read_exposed = true;
            } else if self.ownership.is_borrowed(source.local) {
                let parents = self.owners(source.local)?;
                owners.extend(parents.locals);
                read.extend(parents.read);
                read_exposed |= parents.is_exposed;
            }
            read.push(source);
            let range = Range {
                local,
                site,
                owners: &owners,
                read: &read,
            };
            if !stays_over_range(body, self.ownership, &range, &self.harm, read_exposed) {
                return None;
            }
            all.locals.extend(owners);
            all.read.extend(read);
            all.is_exposed |= read_exposed;
        }
        Some(all)
    }
}

/// Each local `body` holds an entry in that a runtime getter lends out of a
/// collection, with the root local of every collection it is read from: the
/// entry holds no reference, so a change to that collection may free it.
fn entry_collections(body: &Body) -> HashMap<Local, Vec<Local>> {
    let mut entries: HashMap<Local, Vec<Local>> = HashMap::new();
    let terminators = body
        .basic_blocks
        .iter()
        .filter_map(|data| data.terminator.as_ref());
    for terminator in terminators {
        let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = &terminator.kind
        else {
            continue;
        };
        let lends_entry =
            (func.called_runtime_effect()).is_some_and(RuntimeEffect::hands_back_a_borrow);
        if !lends_entry || !destination.projection.is_empty() {
            continue;
        }
        let entry = destination.local;
        let collection = args
            .first()
            .and_then(Operand::place)
            .map(|place| place.local);
        let roots = entries.entry(entry).or_default();
        roots.extend(collection.filter(|&c| c != entry));
    }
    entries
}

/// The locals `owners` names, as a finding spells them.
fn owner_names(owners: &[Local]) -> String {
    owners
        .iter()
        .map(Local::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// How a write reaches a value of some type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Writes {
    /// A reference by design: a write changes what every holder sees, and
    /// asks no count.
    Shared,
    /// A value a write copies first, and a callee writing a parameter of it
    /// copies on entry.
    CopiedOnEntry,
    /// Any other value, which a callee may write in place by its count.
    InPlace,
}

/// How a write reaches a value of type `ty` held in `body`.
fn written_as(body: &Body, ty: &Type) -> Writes {
    if is_reference_by_design(&ty.kind, body.rc_classifier.definitions()) {
        Writes::Shared
    } else if is_copied_on_write(body, ty) {
        Writes::CopiedOnEntry
    } else {
        Writes::InPlace
    }
}

/// Whether `access` to a candidate borrow whose value `writes` reach
/// refuses it.
fn refuses(access: Access, writes: Writes) -> bool {
    match access {
        Access::Rebound
        | Access::CallResult
        | Access::Released
        | Access::AddressTaken
        | Access::Handed => true,
        Access::WrittenThrough | Access::MayBeWritten => writes != Writes::Shared,
        Access::LentToEntryCopy => writes == Writes::InPlace,
        Access::ProjectedRead
        | Access::WholeRead
        | Access::Moved
        | Access::Defined
        | Access::Retained
        | Access::ProjectedRetain
        | Access::Storage
        | Access::ReadByCallee
        | Access::Stored => false,
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
        | Access::LentToEntryCopy
        | Access::MayBeWritten
        | Access::Stored
        | Access::Handed => false,
    }
}

/// Each definition of `local` with the place it copies; `None` when one of
/// them is anything but a copy of a projected place, or, into a temporary the
/// compiler made, of a whole local.
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
            if source.projection.is_empty() && body.local_decls[local.0].is_user_variable {
                return None;
            }
            reads.push(((block, index), source.clone()));
        }
    }
    (!reads.is_empty()).then_some(reads)
}

/// Whether `root` holds a reference a borrow can rely on: a counted local the
/// body owns or a counted parameter other than an `out` one, never a capture,
/// a local holding a device buffer, or one whose address the body takes.
fn is_root(body: &Body, root: Local) -> bool {
    (root.0 > body.arg_count || is_parameter(body, root))
        && !body.captures.env_capture_locals.contains(&root)
        && body.is_counted_local(root)
        && body.local_decls[root.0].device_handle.is_none()
        && !accesses_in(body)
            .any(|(local, access, _)| local == root && access == Access::AddressTaken)
}

/// Whether `local` is a parameter of `body` other than an `out` one.
fn is_parameter(body: &Body, local: Local) -> bool {
    let is_out = (local.0.checked_sub(1))
        .and_then(|index| body.signature.out_params.get(index))
        .copied()
        .unwrap_or(false);
    (1..=body.arg_count).contains(&local.0) && !is_out
}

/// Whether `statement` ends the binding of a parameter the body is lent:
/// its caller's reference still holds the value.
fn ends_lent_parameter(body: &Body, ownership: &OwnershipFacts, statement: &Statement) -> bool {
    matches!(&statement.kind, StatementKind::StorageDead(place, _)
        if place.projection.is_empty()
            && is_parameter(body, place.local)
            && !ownership.is_owned_param(place.local))
}

/// A borrow's range: the borrow, the definition it starts at, and the
/// owners that must stay as they were over it.
struct Range<'r> {
    local: Local,
    site: (usize, usize),
    owners: &'r [Local],
    /// Every place copied out of an owner on the way to the borrow.
    read: &'r [Place],
}

/// Whether, on every path from the definition of the range's local at its
/// site to its last read, or to the end of its storage when that comes
/// first, each of its owners is reached only in
/// ways that leave it as it was, and — when other code may reach the value
/// (`is_exposed`) — every operation that runs is harmless by `harm`. A path
/// reaching the body's end first does not end the range.
fn stays_over_range(
    body: &Body,
    ownership: &OwnershipFacts,
    range: &Range,
    harm: &BodyHarm,
    is_exposed: bool,
) -> bool {
    let Range {
        local,
        site,
        owners,
        ..
    } = *range;
    let loud = is_exposed.then_some(harm);
    let ahead = ReadAhead::of(body, local);
    let mut pending = vec![(site.0, site.1 + 1)];
    let mut visited = HashSet::new();
    'blocks: while let Some((block, from)) = pending.pop() {
        let data = &body.basic_blocks[block];
        for (index, statement) in data.statements.iter().enumerate().skip(from) {
            if ends_local(statement, local) || !ahead.is_read_from(block, index) {
                continue 'blocks;
            }
            if statement_breaks_range(body, ownership, statement, range, loud) {
                return false;
            }
        }
        if !ahead.is_read_from(block, data.statements.len()) {
            continue 'blocks;
        }
        let Some(terminator) = &data.terminator else {
            return false;
        };
        let at = BasicBlock(block);
        let is_loud = loud.is_some_and(|harm| !harm.is_harmless_terminator(at, terminator));
        if matches!(terminator.kind, TerminatorKind::Return)
            || changes_owner(
                credited_accesses(terminator, harm.calls_harmless_program(at, terminator)),
                owners,
            )
            || is_loud
        {
            return false;
        }
        pending.extend(
            (body.successors_while_live(terminator, local).into_iter())
                .filter(|successor| visited.insert(successor.0))
                .map(|successor| (successor.0, 0)),
        );
    }
    true
}

/// Whether `statement`, inside `range`, changes one of its owners, or — when
/// other code may reach the borrowed value (`loud` given) — may release a
/// value another name holds by `loud`'s rule. A write beside every place read
/// out of an owner ([`write_beside`]) changes it only through the value it
/// stores, and is harmless unless what it releases runs a drop hook.
fn statement_breaks_range(
    body: &Body,
    ownership: &OwnershipFacts,
    statement: &Statement,
    range: &Range,
    loud: Option<&BodyHarm>,
) -> bool {
    let beside = write_beside(body, statement, (range.owners, range.read));
    let is_loud = loud.is_some_and(|harm| match &beside {
        Some(write) => !write.is_harmless(harm),
        None => !harm.is_harmless_statement(&statement.kind),
    });
    let accesses = match &beside {
        Some(write) => write.accesses(),
        None => statement_accesses(statement),
    };
    is_loud
        || (!ends_lent_parameter(body, ownership, statement)
            && changes_owner(accesses, range.owners))
}

/// Whether one of `accesses` reaches one of `owners` in a way that does not
/// leave it as it was.
fn changes_owner(accesses: Vec<(Local, Access)>, owners: &[Local]) -> bool {
    accesses
        .into_iter()
        .any(|(accessed, access)| owners.contains(&accessed) && !leaves_owner(access))
}

/// A write into an owner that parts, at a field of its own
/// ([`parts_at_own_field`]), from every place copied out of that owner on the
/// way to a borrow: it leaves the borrow's value where it was.
struct WriteBeside<'s> {
    place: &'s Place,
    /// The value it stores, if it stores one.
    stored: Option<&'s Rvalue>,
    /// Whether it releases the value the place held.
    releases: bool,
}

impl WriteBeside<'_> {
    /// Every local it reaches other than the owner it writes into: the
    /// locals indexing its place, and those its stored value reads.
    fn accesses(&self) -> Vec<(Local, Access)> {
        let mut accesses: Vec<(Local, Access)> = (self.place.index_locals())
            .map(|index| (index, Access::ProjectedRead))
            .collect();
        if let Some(rvalue) = self.stored {
            rvalue_accesses(rvalue, &mut accesses);
        }
        accesses
    }

    /// Whether it is harmless by `harm`: the value it releases, if any, runs
    /// no drop hook.
    fn is_harmless(&self, harm: &BodyHarm) -> bool {
        !self.releases || harm.is_harmless_release(self.place)
    }
}

/// `statement` as a write beside what a borrow reads out of `owners` through
/// the places `read`, or `None` when it is anything else.
fn write_beside<'s>(
    body: &Body,
    statement: &'s Statement,
    (owners, read): (&[Local], &[Place]),
) -> Option<WriteBeside<'s>> {
    let write = match &statement.kind {
        StatementKind::Assign(place, rvalue) => WriteBeside {
            place,
            stored: Some(rvalue),
            releases: false,
        },
        StatementKind::Reassign(place, rvalue) => WriteBeside {
            place,
            stored: Some(rvalue),
            releases: true,
        },
        StatementKind::DecRef(place) => WriteBeside {
            place,
            stored: None,
            releases: true,
        },
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(..)
        | StatementKind::IncRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => return None,
    };
    let root = write.place.local;
    let mut read_out_of_root = read.iter().filter(|place| place.local == root).peekable();
    let is_beside = owners.contains(&root)
        && read_out_of_root.peek().is_some()
        && read_out_of_root.all(|place| parts_at_own_field(body, write.place, place));
    is_beside.then_some(write)
}

/// Where a local is read again, by this check's own access classification:
/// each block's last access to it other than the start or end of its
/// storage or a whole definition, the terminator counting as the position
/// after the last statement, and whether a read follows each block before
/// the local is defined again or its storage ends.
struct ReadAhead {
    last_read: Vec<Option<usize>>,
    read_after: Vec<bool>,
}

/// What a block does to a local first: reads it, or ends it (its storage
/// ends or it is defined whole) before any read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstTouch {
    Read,
    Ended,
}

impl ReadAhead {
    fn of(body: &Body, local: Local) -> Self {
        let mut last_read = Vec::with_capacity(body.basic_blocks.len());
        let mut first_touch = Vec::with_capacity(body.basic_blocks.len());
        for data in &body.basic_blocks {
            let (first, last) = touches(data, local);
            first_touch.push(first);
            last_read.push(last);
        }
        let successors: Vec<Vec<BasicBlock>> = (body.basic_blocks.iter())
            .map(|data| {
                data.terminator
                    .as_ref()
                    .map_or_else(Vec::new, |t| t.successors())
            })
            .collect();
        let mut read_on_entry: Vec<bool> = first_touch
            .iter()
            .map(|t| *t == Some(FirstTouch::Read))
            .collect();
        let mut grew = true;
        while grew {
            grew = false;
            for block in 0..successors.len() {
                let untouched = first_touch[block].is_none();
                if untouched
                    && !read_on_entry[block]
                    && successors[block].iter().any(|s| read_on_entry[s.0])
                {
                    read_on_entry[block] = true;
                    grew = true;
                }
            }
        }
        let read_after = (successors.iter())
            .map(|next| next.iter().any(|s| read_on_entry[s.0]))
            .collect();
        Self {
            last_read,
            read_after,
        }
    }

    /// Whether the local is read at or after position `index` of `block`.
    fn is_read_from(&self, block: usize, index: usize) -> bool {
        self.last_read[block].is_some_and(|last| last >= index) || self.read_after[block]
    }
}

/// Whether `statement` ends `local`'s storage or defines it whole.
fn ends_local(statement: &Statement, local: Local) -> bool {
    matches!(&statement.kind,
        StatementKind::StorageDead(p, _) | StatementKind::Assign(p, _) if *p == Place::new(local))
}

/// What block `data` does to `local` first, and the position of its last
/// read there.
fn touches(data: &BasicBlockData, local: Local) -> (Option<FirstTouch>, Option<usize>) {
    let reads = |accesses: Vec<(Local, Access)>| {
        (accesses.into_iter()).any(|(accessed, access)| {
            accessed == local && !matches!(access, Access::Storage | Access::Defined)
        })
    };
    let mut first = None;
    let mut last = None;
    for (index, statement) in data.statements.iter().enumerate() {
        if reads(statement_accesses(statement)) {
            last = Some(index);
            first.get_or_insert(FirstTouch::Read);
        }
        if ends_local(statement, local) {
            first.get_or_insert(FirstTouch::Ended);
        }
    }
    if (data.terminator.as_ref()).is_some_and(|terminator| reads(terminator_accesses(terminator))) {
        last = Some(data.statements.len());
        first.get_or_insert(FirstTouch::Read);
    }
    (first, last)
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

/// Every access in `body`, with what a call to a function of the program
/// found harmless by `harm` is lent credited as only read
/// ([`credited_accesses`]).
fn credited_accesses_in<'a>(
    body: &'a Body,
    harm: &'a BodyHarm,
) -> impl Iterator<Item = (Local, Access)> + 'a {
    body.basic_blocks
        .iter()
        .enumerate()
        .flat_map(move |(block, data)| {
            let statements = data.statements.iter().flat_map(statement_accesses);
            let terminator = data.terminator.iter().flat_map(move |terminator| {
                let at = BasicBlock(block);
                credited_accesses(terminator, harm.calls_harmless_program(at, terminator))
            });
            statements.chain(terminator)
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
        StatementKind::StorageLive(place) | StatementKind::StorageDead(place, _) => {
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
        StatementKind::Nop | StatementKind::ReadHeader(_) => {}
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

/// How `terminator` reaches each local it names, crediting a call to a
/// harmless function of the program (`calls_harmless`): such a callee stores
/// into nothing but its own locals — a copy it binds on entry of a parameter
/// it writes among them — so what it is lent by copy, at any position and its
/// receiver included, is only read. An owner a borrow relies on stays as it
/// was, and a borrow lent so is never written through by the count its owner
/// holds.
fn credited_accesses(terminator: &Terminator, calls_harmless: bool) -> Vec<(Local, Access)> {
    let mut accesses = terminator_accesses(terminator);
    if calls_harmless {
        for (_, access) in &mut accesses {
            if matches!(access, Access::MayBeWritten | Access::LentToEntryCopy) {
                *access = Access::ReadByCallee;
            }
        }
    }
    accesses
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
    let runtime = func.and_then(Operand::called_runtime_effect);
    for (position, arg) in args.iter().enumerate() {
        let Some(place) = arg.place() else {
            continue;
        };
        let mode = arg_mode(arg_modes, position);
        let access = if gives_its_reference(arg, mode) {
            Access::Handed
        } else if is_lent_to_entry_copy(func, position, arg, mode) {
            Access::LentToEntryCopy
        } else {
            argument_access(runtime, position, mode)
        };
        push_place(&mut accesses, place, access, access);
    }
    accesses
}

/// Whether `arg`, passed `mode`, gives the callee the caller's own reference
/// to a whole local: a `move` to a position the callee owns or whose
/// container keeps the value. Only a copy is funded by a retain, so a borrow,
/// which holds no reference, cannot be handed over this way.
fn gives_its_reference(arg: &Operand, mode: ArgMode) -> bool {
    let Operand::Move(place) = arg else {
        return false;
    };
    let takes_a_reference = match mode {
        ArgMode::Owned | ArgMode::Donated => true,
        ArgMode::Borrowed | ArgMode::Out => false,
    };
    place.projection.is_empty() && takes_a_reference
}

/// Whether a call to `func` lends `arg`, at `position` and passed `mode`, as
/// a whole local by copy to a parameter its callee copies on entry before a
/// write: one of a function of the program, or of a method past its
/// receiver.
fn is_lent_to_entry_copy(
    func: Option<&Operand>,
    position: usize,
    arg: &Operand,
    mode: ArgMode,
) -> bool {
    let Operand::Copy(place) = arg else {
        return false;
    };
    let Some(symbol) = func.and_then(Operand::called_symbol) else {
        return false;
    };
    let is_copying_parameter = symbol.is_plain_function() || (symbol.is_method() && position > 0);
    place.projection.is_empty() && mode == ArgMode::Borrowed && is_copying_parameter
}

/// How a call reaches its argument at `position`, passed `mode`, when its
/// callee is a runtime entry point doing `runtime` (`None` for any other).
fn argument_access(runtime: Option<&RuntimeEffect>, position: usize, mode: ArgMode) -> Access {
    let checked_or_kept = runtime.is_some_and(|effect| {
        effect.is_copy_on_write()
            || effect.reads_count
            || effect.handed_over_positions().contains(&position)
    });
    // Whether the argument is spelled as a copy or a move changes nothing:
    // the entry point's declaration alone says it only reads it.
    let only_read = runtime.is_some_and(|effect| effect.only_reads(position));
    match mode {
        ArgMode::Borrowed | ArgMode::Owned | ArgMode::Out if checked_or_kept => Access::Handed,
        ArgMode::Borrowed if only_read => Access::ReadByCallee,
        ArgMode::Borrowed | ArgMode::Owned => Access::MayBeWritten,
        ArgMode::Out => Access::Handed,
        ArgMode::Donated => Access::Stored,
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
