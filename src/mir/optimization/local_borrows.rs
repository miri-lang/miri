// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Borrowed locals: a managed value read out of a place the body owns or is
//! lent takes no reference while nothing can change or release that place.
//!
//! A local `l = copy p`, where `p` is reached from a root `r` — a local the
//! body owns, or a parameter — through value steps — an element of a list or
//! an array, a field of a struct, a tuple or an optional, an enum's payload —
//! and class fields, reads a value `r` holds. While `r` and everything on the
//! way stays as it was, `r`'s reference keeps that value alive, so `l` can
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
//!   uniqueness check on it and every move of it into a call is refused, and so
//!   is every hand-off to a callee, except a whole `l` of a type a write copies
//!   (`type_facts::unsharing`) lent to a function of the program, or to a method
//!   past its receiver: a body writing such a parameter binds a copy of it on
//!   entry (`lowering::written_parameters`), which takes a reference of its
//!   own, so the write copies. A method's receiver gets no such copy — a write
//!   through it relies on the caller's copy-on-write, or checks that it arrived
//!   unique (`RECEIVER_UNIQUE`), which a borrowed count would answer yes to.
//! - **Its root holds a reference.** The root is a counted local the body owns,
//!   or a parameter — lent, taken over, or the receiver — but never an `out`
//!   parameter, a capture or a local whose address is taken; every step from
//!   it to `p` is a value step or a field of a class instance, never a
//!   dereference.
//! - **Its root is not touched while it borrows.** On every path from a
//!   definition of `l` to the end of `l`'s storage, the root is only read
//!   through a projection, or lent to a call by copy: never written, moved,
//!   copied whole, passed owned or `out`, released, or ended — though the end
//!   of a lent parameter's binding leaves its caller's reference in place. A
//!   borrow read out of another borrowed local inherits that local's root, so
//!   its root's root must stay as well, and the end of its root's storage is a
//!   change. A path that reaches the end of the body before `l`'s storage ends
//!   is refused.
//! - **Nothing that may release a value another name holds runs while it
//!   borrows, when other code may reach what it reads.** A value held behind a
//!   class field is reached by every holder of the instance, and a parameter's
//!   value by its caller and whatever else the caller lent it to, so a method
//!   can clear the field, or empty the list, without naming the root. Such a
//!   borrow — and every borrow read out of one — is refused when its range
//!   holds an operation `analysis::heap_effects` finds loud: a store through a
//!   class field or into a shared root, a release that may run a drop hook, a
//!   virtual call, a closure call, a loud runtime call, or a call to a function
//!   of the program not recorded quiet (`Body::quiet_calls`). That rule ignores
//!   which value an operation touches, so an alias of the root changes nothing.
//!   A value only the root's own value holds is reached by nothing else: any
//!   other holder of it holds its own reference and copies before writing.
//! - **An entry a collection lends is no owner on its own.** A runtime getter
//!   that hands back an entry as a borrow (`runtime_fns::hands_back_a_borrow`,
//!   `m[k]` on a map) leaves the local it defines holding no reference: the
//!   collection's keeps the entry alive. A borrow read out of such a local
//!   counts the locals holding the collection among its owners — rebinding,
//!   emptying or releasing the map ends it — and is reached by other code,
//!   since whatever else holds the collection can replace it.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, STRING_TYPE_NAME};
use crate::mir::analysis::heap_effects::{borrowed_entries, BodyNoise, Phase};
use crate::mir::optimization::perceus::{place_declared_type, Perceus, PerceusContext};
use crate::mir::place::PlaceContext;
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::type_facts::DropHookReach;
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
    let exposed_sites = retain_rooted_reads(body, ctx, managed_locals, &mut reads);
    let entries = entry_collections(body);
    let hooks = DropHookReach::new(definitions(ctx));
    let noise = (!exposed_sites.is_empty() || !entries.is_empty())
        .then(|| BodyNoise::of(body, Phase::Lowered, &hooks, Some(&body.quiet_calls)));
    let mut settled = Settled {
        body,
        reads: &reads,
        exposed_sites: &exposed_sites,
        entries: &entries,
        noise: noise.as_ref(),
        decided: HashMap::new(),
        owners: HashMap::new(),
        exposed: HashSet::new(),
    };
    for &local in reads.keys() {
        settled.borrows(local);
    }
    settled.owners
}

/// Keeps the reads in `reads` whose candidate reads, from a root that holds a
/// reference, through steps a borrow can rely on, dropping every candidate
/// with a read that does not. Returns the sites of the reads other code may
/// reach: through a class field, or out of a parameter.
fn retain_rooted_reads(
    body: &Body,
    ctx: &PerceusContext,
    managed_locals: &HashSet<Local>,
    reads: &mut Reads,
) -> HashSet<Site> {
    let roots: HashSet<Local> = reads
        .values()
        .flatten()
        .map(|(_, source)| source.local)
        .collect();
    let address_taken = address_taken_locals(body, &roots);
    let mut exposed_sites = HashSet::new();
    reads.retain(|&local, sources| {
        sources.iter().all(|(site, source)| {
            let is_root = source.local != local
                && is_root(body, source.local, managed_locals)
                && !address_taken.contains(&source.local);
            let reach = is_root
                .then(|| reach_of(source, |p| place_declared_type(p, ctx), definitions(ctx)))
                .flatten();
            if reach == Some(Reach::ThroughField) || is_parameter(body, source.local) {
                exposed_sites.insert(*site);
            }
            reach.is_some()
        })
    });
    exposed_sites
}

/// Each local `body` holds an entry in that a runtime getter hands back as a
/// borrow of a collection, with the root local of every collection it reads
/// the entry from. Such a local holds no reference: the collection's does.
fn entry_collections(body: &Body) -> HashMap<Local, Vec<Local>> {
    let mut entries: HashMap<Local, Vec<Local>> = HashMap::new();
    for (entry, collection) in borrowed_entries(body) {
        let roots = entries.entry(entry).or_default();
        if let Some(place) = collection {
            if !roots.contains(&place.local) {
                roots.push(place.local);
            }
        }
    }
    entries
}

/// Decides, parents first, which candidates borrow: a candidate read out of
/// another candidate depends on whether that one borrows, since a borrowed
/// parent's own root must then stay as well.
struct Settled<'b> {
    body: &'b Body,
    reads: &'b Reads,
    /// The reads other code may reach what they read through.
    exposed_sites: &'b HashSet<Site>,
    /// Each entry a runtime getter hands back as a borrow, with the locals
    /// holding the collections it may come from.
    entries: &'b HashMap<Local, Vec<Local>>,
    /// What may release a value another name holds, which ends a borrow of
    /// a value other code may reach; `None` when the body reads none.
    noise: Option<&'b BodyNoise<'b>>,
    /// Each candidate decided so far; `None` while it is being decided, so a
    /// candidate read, around a loop, out of itself is refused.
    decided: HashMap<Local, Option<bool>>,
    /// Each candidate that borrows, with the owners of what it holds.
    owners: HashMap<Local, Vec<Local>>,
    /// The borrows other code may reach what they hold through, directly or
    /// through a borrowed parent.
    exposed: HashSet<Local>,
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
        let mut is_exposed = false;
        for (site, source) in reads.get(&local).into_iter().flatten() {
            let Some((owners, parent_exposed)) = self.owners_of(source.local) else {
                answer = None;
                continue;
            };
            let read_exposed = parent_exposed || self.exposed_sites.contains(site);
            let watch = Watch {
                body: self.body,
                owners: &owners,
                noise: self.noise.filter(|_| read_exposed),
            };
            if (read_exposed && self.noise.is_none()) || !watch.stays(local, *site) {
                answer = answer.and(Some(false));
            }
            is_exposed |= read_exposed;
            for owner in owners {
                if !all_owners.contains(&owner) {
                    all_owners.push(owner);
                }
            }
        }
        if answer == Some(true) {
            self.owners.insert(local, all_owners);
            if is_exposed {
                self.exposed.insert(local);
            }
        }
        self.decided.insert(local, Some(answer == Some(true)));
        answer
    }

    /// `root` and, while it is itself a borrowed read, the owners of what it
    /// reads — or, for an entry a collection lends, the locals holding that
    /// collection: every local whose change could release what a borrow out
    /// of `root` holds; with whether other code may reach what `root` holds.
    /// An entry is reached by whatever else holds its collection. `None` on a
    /// cycle of reads.
    fn owners_of(&mut self, root: Local) -> Option<(Vec<Local>, bool)> {
        let mut owners = vec![root];
        if let Some(collections) = self.entries.get(&root) {
            owners.extend(collections.iter().filter(|&&c| c != root));
            return Some((owners, true));
        }
        if !self.reads.contains_key(&root) || !self.borrows(root)? {
            return Some((owners, false));
        }
        let reads = self.reads;
        for (_, source) in reads.get(&root).into_iter().flatten() {
            for owner in self.owners_of(source.local)?.0 {
                if !owners.contains(&owner) {
                    owners.push(owner);
                }
            }
        }
        Some((owners, self.exposed.contains(&root)))
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
        MirType::Custom(name) if body.has_drop_types.contains(name.as_str()));
    !has_drop_hook && decl.device_handle.is_none()
}

/// Whether `root` holds a reference a borrow can rely on: a counted local the
/// body owns, or a counted parameter — lent, taken over, or the receiver —
/// but not the return slot, an `out` parameter, a capture, or a local holding
/// a device buffer.
fn is_root(body: &Body, root: Local, managed_locals: &HashSet<Local>) -> bool {
    let is_counted = if is_parameter(body, root) {
        body.is_counted_local(root)
    } else {
        root.0 > body.arg_count && managed_locals.contains(&root)
    };
    is_counted
        && !body.env_capture_locals.contains(&root)
        && body.local_decls[root.0].device_handle.is_none()
}

/// Whether `local` is a parameter of `body` other than an `out` one.
fn is_parameter(body: &Body, local: Local) -> bool {
    (1..=body.arg_count).contains(&local.0)
        && !body.out_params.get(local.0 - 1).copied().unwrap_or(false)
}

/// Whether `statement` ends the binding of a parameter `body` is lent, whose
/// value its caller's reference keeps.
fn ends_lent_parameter(body: &Body, statement: &Statement) -> bool {
    matches!(&statement.kind, StatementKind::StorageDead(place)
        if place.projection.is_empty()
            && is_parameter(body, place.local)
            && !body.is_owned_param(place.local))
}

/// The candidates something in `body` uses in a way a borrow cannot carry:
/// a definition that is not a read, a rebinding, a release by hand, a write
/// through it, a runtime entry point keeping or checking it, an `out`
/// argument, a kernel launch, or — for a value that is not a reference by
/// design — a hand-off to a callee that may write it in place.
fn refused_locals(body: &Body, ctx: &PerceusContext, reads: &Reads) -> HashSet<Local> {
    let reads_at: HashSet<(Local, Site)> = reads
        .iter()
        .flat_map(|(&local, sources)| sources.iter().map(move |(site, _)| (local, *site)))
        .collect();
    let is_reference =
        |local: Local| has_reference_semantics(&ctx.local_decls[local.0].ty.kind, definitions(ctx));
    let is_copied = |local: Local| is_copied_on_write(body, &ctx.local_decls[local.0].ty);
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
                refused_by_terminator(terminator, is_reference, is_copied)
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

/// The locals `rvalue` takes the address of or hands to a GPU operation.
fn refused_by_rvalue(rvalue: &Rvalue) -> Vec<Local> {
    match rvalue {
        Rvalue::Ref(place) => vec![place.local],
        Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => {
            let mut mentions = Mentions::default();
            mentions.visit_rvalue(rvalue, BasicBlock(0));
            mentions.locals
        }
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
/// `is_reference` — hands to any callee not known to only read it, other than
/// one copying it on entry before a write ([`lends_to_entry_copy`], for a
/// local `is_copied` holds a value a write copies). A method's receiver gets
/// no such copy and is written in place by the count its caller hands it, and
/// a borrowed value's count is its owner's.
fn refused_by_terminator(
    terminator: &Terminator,
    is_reference: impl Fn(Local) -> bool,
    is_copied: impl Fn(Local) -> bool,
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
        let copies_on_entry = is_copied(place.local)
            && lends_to_entry_copy(func, position, arg, arg_mode(arg_modes, position));
        let may_write_a_value =
            !is_reference(place.local) && !only_reads(func, position) && !copies_on_entry;
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

/// Whether the call to `func` lends `arg`, a whole local by copy and `mode`
/// borrowed, to a parameter its callee binds a copy of on entry before
/// writing it: any parameter of a function of the program, and any of a
/// method's past its receiver. A closure, a virtual call and every other
/// symbol are not known to.
fn lends_to_entry_copy(
    func: Option<&Operand>,
    position: usize,
    arg: &Operand,
    mode: ArgMode,
) -> bool {
    let is_whole_copy = matches!(arg, Operand::Copy(place) if place.projection.is_empty());
    let copies_parameter = func
        .and_then(Operand::called_symbol)
        .is_some_and(|symbol| symbol.is_plain_function() || (symbol.is_method() && position > 0));
    is_whole_copy && mode == ArgMode::Borrowed && copies_parameter
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
    /// Something in the block changes an owner, or may release a value
    /// another name holds where the borrow's value is one other code reaches.
    Changed,
    /// Nothing ends the range or changes anything; it carries on into the
    /// block's successors.
    Continues(Vec<BasicBlock>),
}

/// What a borrow's range must leave as it was.
struct Watch<'w> {
    body: &'w Body,
    owners: &'w [Local],
    /// For a borrow other code may reach what it holds through: the rule
    /// finding each operation that may release a value another name holds,
    /// every one of which ends it.
    noise: Option<&'w BodyNoise<'w>>,
}

impl Watch<'_> {
    /// Whether nothing changes on any path from the definition of `local` at
    /// `site` to the end of its storage.
    fn stays(&self, local: Local, site: Site) -> bool {
        let mut pending = vec![(site.0, site.1 + 1)];
        let mut entered = HashSet::new();
        while let Some((block, from)) = pending.pop() {
            match self.segment(block, local, from) {
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

    /// How block `block`, from statement `from` on, extends the range of
    /// `local`.
    fn segment(&self, block: usize, local: Local, from: usize) -> Segment {
        let data = &self.body.basic_blocks[block];
        for statement in data.statements.iter().skip(from) {
            if ends_range(statement, local) {
                return Segment::Ended;
            }
            if self.is_changed_by(statement) {
                return Segment::Changed;
            }
        }
        match data.terminator.as_ref() {
            Some(terminator) if !self.is_changed_by_terminator(block, terminator) => {
                match &terminator.kind {
                    TerminatorKind::Return => Segment::Changed,
                    TerminatorKind::Goto { .. }
                    | TerminatorKind::SwitchInt { .. }
                    | TerminatorKind::Unreachable
                    | TerminatorKind::Call { .. }
                    | TerminatorKind::VirtualCall { .. }
                    | TerminatorKind::GpuLaunch { .. } => {
                        Segment::Continues(terminator.successors())
                    }
                }
            }
            Some(_) | None => Segment::Changed,
        }
    }

    fn is_changed_by(&self, statement: &Statement) -> bool {
        if ends_lent_parameter(self.body, statement) {
            return false;
        }
        statement_changes(statement, self.owners)
            || self
                .noise
                .is_some_and(|noise| noise.is_loud_statement(&statement.kind))
    }

    fn is_changed_by_terminator(&self, block: usize, terminator: &Terminator) -> bool {
        terminator_changes(terminator, self.owners)
            || self
                .noise
                .is_some_and(|noise| noise.is_loud_terminator(BasicBlock(block), terminator))
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
/// point that only reads it. A method writes its receiver in place, by the
/// count its caller hands it (`RECEIVER_UNIQUE`), and so may free what a
/// borrow reads. An owner lent to any other callee is refused too, though a
/// function of the program copies a parameter it writes on entry.
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

/// Where a place read out of a root is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// In the root's own value: every step is a value step.
    InValue,
    /// Behind a field of a class instance on the way, which every holder of
    /// the instance reaches and may replace.
    ThroughField,
}

/// How `place` is reached from its root, through value steps — an element of
/// a list or an array, a field of a struct, a tuple, an optional or an enum —
/// and fields of class instances; `None` for a place with no step, or with a
/// dereference or a field of a value whose holder is not known. `declared`
/// gives the declared type of each place on the way.
pub(crate) fn reach_of(
    place: &Place,
    declared: impl Fn(&Place) -> Option<Type>,
    definitions: &HashMap<String, TypeDefinition>,
) -> Option<Reach> {
    if place.projection.is_empty() {
        return None;
    }
    place
        .projection
        .iter()
        .enumerate()
        .try_fold(Reach::InValue, |reach, (depth, step)| {
            let container = Place {
                local: place.local,
                projection: place.projection[..depth].to_vec(),
            };
            let ty = declared(&container)?;
            if is_value_step(&ty.kind, step, definitions) {
                Some(reach)
            } else {
                let is_class_field =
                    matches!(step, PlaceElem::Field(_)) && is_class_instance(&ty.kind, definitions);
                is_class_field.then_some(Reach::ThroughField)
            }
        })
}

/// Whether a value of `kind` is an instance of a class of the program, other
/// than a built-in collection.
fn is_class_instance(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> bool {
    let TypeKind::Custom(name, _) = kind else {
        return false;
    };
    kind.as_builtin_collection().is_none()
        && matches!(definitions.get(name), Some(TypeDefinition::Class(_)))
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
