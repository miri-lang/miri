// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every collection a body records as stable over a loop keeps
//! its header there.
//!
//! Code generation reads a recorded collection's data pointer and length once,
//! in the record's preheader, and uses them at every access inside the
//! record's blocks. That is sound only while every path into those blocks
//! passes through the preheader, the local keeps its value throughout, and
//! nothing there can change a header: no call outside the runtime entry
//! points that leave headers unchanged — among them a copy of an array or a
//! list whose elements run no `clone` method of the program — except a call
//! to a function of the program the verifiers' own rule finds harmless
//! ([`super::callee_effects`]), or one handed nothing that may hold the
//! collection while it holds a reference of its own — worked out here by a
//! cruder walk of its own, never the alias reach the pass asks — no store
//! through a dereference, no
//! release that may run a drop hook, and — for a parameter the body borrows —
//! no release, other than through the parameter, of a value that may hold a
//! collection of its type. Each record is re-derived here from the body
//! alone, without the pass's loop search, and every way it fails is
//! reported, as is a block two reads of one collection cover.

use super::VerificationViolation;
use crate::ast::types::BuiltinCollectionKind;
use crate::mir::analysis::header_reach::{may_hold_type, runtime_call_leaves_headers};
use crate::mir::facts::OwnershipFacts;
use crate::mir::header_reads::stable_collections;
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue, StableCollection, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use std::collections::{HashMap, HashSet};

/// Every way a header read ([`crate::mir::header_reads::stable_collections`]) fails to keep its
/// collection's header. `harmless_calls` are the body's calls to the program
/// found harmless by [`super::callee_effects`]; `None` takes every such call
/// as one that may change a header.
pub fn verify_stable_collections(
    body: &Body,
    ownership: &OwnershipFacts,
    harmless_calls: Option<&HashSet<BasicBlock>>,
) -> Vec<VerificationViolation> {
    let records = stable_collections(body);
    if records.is_empty() {
        return Vec::new();
    }
    let checker = RecordCheck::of(body, ownership, harmless_calls);
    (records.iter())
        .flat_map(|record| checker.failures(record))
        .chain(overlaps(&records))
        .map(|(local, message)| VerificationViolation {
            local,
            local_name: display_name(body, local),
            message,
        })
        .collect()
}

/// Every block two reads of one collection's header cover: code generation
/// takes one read per block and collection, so a second is a choice MIR
/// left open.
fn overlaps(records: &[StableCollection]) -> Vec<(Local, String)> {
    let mut covered: HashMap<(BasicBlock, Local), BasicBlock> = HashMap::new();
    let mut found = Vec::new();
    for record in records {
        for block in &record.blocks {
            if let Some(first) = covered.insert((*block, record.local), record.preheader) {
                found.push((
                    record.local,
                    format!(
                        "`{}` is read once in both {first} and {} for {block}",
                        record.local, record.preheader
                    ),
                ));
            }
        }
    }
    found
}

fn display_name(body: &Body, local: Local) -> String {
    (body.local_decls.get(local.0))
        .and_then(|decl| decl.name.as_ref())
        .map_or_else(|| local.to_string(), |name| name.to_string())
}

/// The body every record is checked against, with the type facts its
/// releases are read by.
struct RecordCheck<'a> {
    body: &'a Body,
    ownership: &'a OwnershipFacts,
    places: PlaceTypes<'a>,
    hooks: DropHookReach<'a>,
    predecessors: Vec<Vec<usize>>,
    harmless_calls: Option<&'a HashSet<BasicBlock>>,
}

impl<'a> RecordCheck<'a> {
    fn of(
        body: &'a Body,
        ownership: &'a OwnershipFacts,
        harmless_calls: Option<&'a HashSet<BasicBlock>>,
    ) -> Self {
        let mut predecessors = vec![Vec::new(); body.basic_blocks.len()];
        for (index, data) in body.basic_blocks.iter().enumerate() {
            for successor in data.terminator.iter().flat_map(Terminator::successors) {
                if let Some(list) = predecessors.get_mut(successor.0) {
                    list.push(index);
                }
            }
        }
        Self {
            body,
            ownership,
            places: PlaceTypes::of(body),
            hooks: DropHookReach::new(body.rc_classifier.definitions()),
            predecessors,
            harmless_calls,
        }
    }

    /// Every failure of `record`, each with the local it concerns.
    fn failures(&self, record: &StableCollection) -> Vec<(Local, String)> {
        let local = record.local;
        let mut found: Vec<String> = Vec::new();
        found.extend(self.binding_failure(local));
        found.extend(self.shape_failures(record));
        for block in &record.blocks {
            found.extend(self.block_failures(record, block.0));
        }
        found
            .into_iter()
            .map(|message| {
                (
                    local,
                    format!(
                        "`{local}` is recorded stable over {}, but {message}",
                        record.header
                    ),
                )
            })
            .collect()
    }

    /// Why `local` cannot be cached at all, if it cannot.
    fn binding_failure(&self, local: Local) -> Option<String> {
        let body = self.body;
        if body.is_gpu() {
            return Some("a GPU body keeps no collection header".to_string());
        }
        let Some(decl) = body.local_decls.get(local.0) else {
            return Some("the body declares no such local".to_string());
        };
        let is_indexed_kind = matches!(
            decl.ty.kind.as_builtin_collection(),
            Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
        );
        if !is_indexed_kind {
            return Some(format!(
                "it holds `{}`, which has no element header",
                decl.ty
            ));
        }
        if decl.device_handle.is_some() {
            return Some("it is a gpu-resident binding".to_string());
        }
        if body.env_capture_locals.contains(&local) {
            return Some("it is a captured binding".to_string());
        }
        let is_address_taken = body
            .basic_blocks
            .iter()
            .flat_map(|data| &data.statements)
            .any(|statement| match &statement.kind {
                StatementKind::Assign(_, Rvalue::Ref(place))
                | StatementKind::Reassign(_, Rvalue::Ref(place)) => place.local == local,
                StatementKind::Assign(..)
                | StatementKind::Reassign(..)
                | StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop
                | StatementKind::ReadHeader(_) => false,
            });
        is_address_taken.then(|| "its address is taken".to_string())
    }

    /// How the record's preheader and blocks fail to fence the loop off.
    fn shape_failures(&self, record: &StableCollection) -> Vec<String> {
        let body = self.body;
        let mut found = Vec::new();
        let in_record = |block: usize| record.blocks.iter().any(|b| b.0 == block);
        match body.basic_blocks.get(record.preheader.0) {
            None => found.push(format!("its preheader {} does not exist", record.preheader)),
            Some(data) => {
                let jumps_to_header = matches!(
                    data.terminator.as_ref().map(|terminator| &terminator.kind),
                    Some(TerminatorKind::Goto { target }) if *target == record.header
                );
                let only_reads_headers = (data.statements.iter())
                    .all(|statement| matches!(statement.kind, StatementKind::ReadHeader(_)));
                if !only_reads_headers || !jumps_to_header {
                    found.push(format!(
                        "its preheader {} is not a jump to the header that only reads headers",
                        record.preheader
                    ));
                }
            }
        }
        if !in_record(record.header.0) || in_record(record.preheader.0) || in_record(0) {
            found.push(
                "its blocks hold the entry block, its preheader or not its header".to_string(),
            );
        }
        for block in &record.blocks {
            let preds = self.predecessors.get(block.0).into_iter().flatten();
            for pred in preds {
                let is_fenced =
                    in_record(*pred) || (*block == record.header && *pred == record.preheader.0);
                if !is_fenced {
                    found.push(format!(
                        "its preheader {} does not dominate {block}, which is entered from bb{pred}",
                        record.preheader
                    ));
                }
            }
        }
        found
    }

    /// What in `block` may change the recorded collection's header.
    fn block_failures(&self, record: &StableCollection, block: usize) -> Vec<String> {
        let Some(data) = self.body.basic_blocks.get(block) else {
            return vec![format!("bb{block} does not exist")];
        };
        let mut found: Vec<String> = (data.statements.iter().enumerate())
            .flat_map(|(position, statement)| {
                let at = BasicBlock(block);
                let failure = self.statement_failure(record.local, at, position, statement);
                failure
                    .into_iter()
                    .chain(self.borrow_failure(record.local, at, statement))
            })
            .collect();
        match &data.terminator {
            None => found.push(format!("bb{block} has no terminator")),
            Some(terminator) => {
                found.extend(self.terminator_failure(record.local, block, terminator))
            }
        }
        found
    }

    /// What `statement`, at `position` of `block`, does that may change the
    /// header of `local`'s collection.
    fn statement_failure(
        &self,
        local: Local,
        block: BasicBlock,
        position: usize,
        statement: &Statement,
    ) -> Option<String> {
        match &statement.kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
                if place.local == local && place.projection.is_empty() {
                    return (!self.is_round_trip(local, block.0, position))
                        .then(|| format!("{block} gives it another value"));
                }
                if place.projection.contains(&PlaceElem::Deref) {
                    return Some(format!("{block} stores through a dereference"));
                }
                (!place.projection.is_empty() && self.may_run_hook(place)).then(|| {
                    format!("{block} overwrites a value whose release may run a drop hook")
                })
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                if place.local == local && place.projection.is_empty() {
                    return Some(format!("{block} releases it"));
                }
                self.may_run_hook(place)
                    .then(|| format!("{block} releases a value whose release may run a drop hook"))
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                (place.local == local).then(|| format!("{block} starts or ends its storage"))
            }
            StatementKind::IncRef(_) | StatementKind::Nop | StatementKind::ReadHeader(_) => None,
        }
    }

    /// What `statement` releases that may free the collection a borrowed
    /// parameter `local` points at.
    fn borrow_failure(
        &self,
        local: Local,
        block: BasicBlock,
        statement: &Statement,
    ) -> Option<String> {
        let is_borrowed =
            (1..=self.body.arg_count).contains(&local.0) && !self.ownership.is_owned_param(local);
        let target = self.body.local_decls.get(local.0).map(|decl| &decl.ty)?;
        let released = match &statement.kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
                Some(place).filter(|place| !place.projection.is_empty())
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => Some(place),
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::Nop
            | StatementKind::ReadHeader(_) => None,
        }?;
        let may_free = is_borrowed
            && released.local != local
            && self
                .places
                .declared(released)
                .is_none_or(|ty| may_hold_type(&ty, target));
        may_free.then(|| format!("{block} may release the list it borrows"))
    }

    /// What `terminator`, ending `block`, does that may change the header of
    /// `local`'s collection.
    fn terminator_failure(
        &self,
        local: Local,
        block: usize,
        terminator: &Terminator,
    ) -> Option<String> {
        let rebinds = terminator
            .written_places()
            .any(|place| place.local == local && place.projection.is_empty());
        if rebinds {
            return Some(format!("bb{block} gives it another value"));
        }
        match &terminator.kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
            TerminatorKind::Call { func, args, .. } => match func.called_runtime_effect() {
                Some(effect) => {
                    (!runtime_call_leaves_headers(self.body, effect, args)).then(|| {
                        let name = func.called_runtime_name().unwrap_or_default();
                        format!("bb{block} calls `{name}`, which may change a header")
                    })
                }
                None => self.program_call_failure(local, block, terminator),
            },
            TerminatorKind::VirtualCall { .. } => Some(format!(
                "bb{block} calls through a vtable, which may change a header"
            )),
            TerminatorKind::GpuLaunch { .. } => Some(format!(
                "bb{block} launches a kernel, which may change a header"
            )),
        }
    }

    /// What the call to a function of the program ending `block` may do to
    /// the header of `local`'s collection.
    ///
    /// A callee found harmless changes no header. Any other callee changes
    /// this one only by writing the collection in place through a name that
    /// holds it without a reference of its own — a write through a name
    /// holding one copies first while another reference lives. So the call
    /// also leaves the header when `local` holds a reference of its own and
    /// the call is handed, and writes its result to, nothing rooted at
    /// `local` or at a local a value of `local` was copied or moved into.
    fn program_call_failure(
        &self,
        local: Local,
        block: usize,
        terminator: &Terminator,
    ) -> Option<String> {
        let is_harmless =
            (self.harmless_calls).is_some_and(|calls| calls.contains(&BasicBlock(block)));
        if is_harmless {
            return None;
        }
        if let Some(reason) = self.uncounted_reason(local) {
            return Some(format!(
                "bb{block} calls a function of the program while {reason}"
            ));
        }
        let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = &terminator.kind
        else {
            return None;
        };
        let holders = self.holders_of(local);
        let is_handed = std::iter::once(func)
            .chain(args)
            .filter_map(Operand::place)
            .chain(std::iter::once(destination))
            .any(|place| holders.contains(&place.local));
        is_handed.then(|| {
            format!("bb{block} calls a function of the program handed something that may hold it")
        })
    }

    /// Why `local` may hold its collection without a reference of its own,
    /// worded to follow "while"; `None` when it holds one.
    fn uncounted_reason(&self, local: Local) -> Option<String> {
        let body = self.body;
        if !body.is_counted_local(local) {
            return Some("it is not reference counted".to_string());
        }
        if self.ownership.is_borrowed(local) {
            return Some("it holds no reference of its own".to_string());
        }
        if (1..=body.arg_count).contains(&local.0) && !self.ownership.is_owned_param(local) {
            return Some("it is a parameter the body borrows".to_string());
        }
        (body.basic_blocks.iter().enumerate()).find_map(|(block, data)| {
            let uncounted = (data.statements.iter()).any(|statement| {
                let (StatementKind::Assign(place, Rvalue::Use(operand))
                | StatementKind::Reassign(place, Rvalue::Use(operand))) = &statement.kind
                else {
                    return false;
                };
                let Some(source) = operand.place() else {
                    return false;
                };
                *place == Place::new(local)
                    && !self.moves_an_owned_value(operand, source)
                    && !is_retained_in(&data.statements, source, local)
            });
            uncounted.then(|| format!("bb{block} gives it a value it holds no reference to"))
        })
    }

    /// Whether `operand` moves the whole of `source`, which the body owns
    /// outright: a parameter it takes over, or a local that is neither a
    /// capture nor a borrow.
    fn moves_an_owned_value(&self, operand: &Operand, source: &Place) -> bool {
        let body = self.body;
        let root = source.local;
        let is_owned = if (1..=body.arg_count).contains(&root.0) {
            self.ownership.is_owned_param(root)
        } else {
            !body.env_capture_locals.contains(&root) && !self.ownership.is_borrowed(root)
        };
        matches!(operand, Operand::Move(_)) && source.projection.is_empty() && is_owned
    }

    /// `local` and every local a value of it is copied or moved into, through
    /// any chain of such copies, and every borrow read out of one of them. A
    /// value read out of a holder through a projection is a holder only when
    /// its type may hold a collection of `local`'s type.
    fn holders_of(&self, local: Local) -> HashSet<Local> {
        let body = self.body;
        let target = body.local_decls.get(local.0).map(|decl| &decl.ty);
        let may_hold = |source: &Place| {
            source.projection.is_empty()
                || (self.places.declared(source))
                    .is_none_or(|ty| target.is_none_or(|target| may_hold_type(&ty, target)))
        };
        let mut holders = HashSet::from([local]);
        loop {
            let before = holders.len();
            let statements = body.basic_blocks.iter().flat_map(|data| &data.statements);
            for statement in statements {
                let (StatementKind::Assign(dest, Rvalue::Use(operand))
                | StatementKind::Reassign(dest, Rvalue::Use(operand))) = &statement.kind
                else {
                    continue;
                };
                let holds = operand
                    .place()
                    .is_some_and(|source| holders.contains(&source.local) && may_hold(source));
                if holds {
                    holders.insert(dest.local);
                }
            }
            for (borrow, owners) in &self.ownership.borrowed_locals {
                if owners.iter().any(|owner| holders.contains(owner)) {
                    holders.insert(*borrow);
                }
            }
            if holders.len() == before {
                return holders;
            }
        }
    }

    fn may_run_hook(&self, place: &Place) -> bool {
        self.places
            .declared(place)
            .is_none_or(|ty| self.hooks.may_run_drop_hook(&ty))
    }

    /// Whether `local = move t`, at `position` of `block`, hands `local`
    /// back the value `t = move local` took out of it immediately before.
    fn is_round_trip(&self, local: Local, block: usize, position: usize) -> bool {
        let Some(data) = self.body.basic_blocks.get(block) else {
            return false;
        };
        let Some(temp) = data
            .statements
            .get(position)
            .and_then(|s| whole_move(s).filter(|(to, _)| *to == local))
            .map(|(_, from)| from)
        else {
            return false;
        };
        let earlier = data.statements.get(..position).unwrap_or_default();
        let last_write = earlier.iter().rev().find(|statement| {
            statement.written_place().is_some_and(|place| {
                place.projection.is_empty() && (place.local == temp || place.local == local)
            })
        });
        if let Some(statement) = last_write {
            return whole_move(statement) == Some((temp, local));
        }
        let [pred] = self
            .predecessors
            .get(block)
            .map(Vec::as_slice)
            .unwrap_or_default()
        else {
            return false;
        };
        let Some(pred_data) = self.body.basic_blocks.get(*pred) else {
            return false;
        };
        let falls_through = matches!(
            pred_data.terminator.as_ref().map(|terminator| &terminator.kind),
            Some(TerminatorKind::Goto { target }) if target.0 == block
        );
        falls_through && pred_data.statements.last().and_then(whole_move) == Some((temp, local))
    }
}

/// `(to, from)` when `statement` is `to = move from`, both whole locals.
fn whole_move(statement: &Statement) -> Option<(Local, Local)> {
    match &statement.kind {
        StatementKind::Assign(place, Rvalue::Use(Operand::Move(source)))
        | StatementKind::Reassign(place, Rvalue::Use(Operand::Move(source)))
            if place.projection.is_empty() && source.projection.is_empty() =>
        {
            Some((place.local, source.local))
        }
        StatementKind::Assign(..)
        | StatementKind::Reassign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => None,
    }
}

/// Whether `statements` retain `source` or `local` anywhere: a copy of
/// `source` into `local` in the same block is then funded.
fn is_retained_in(statements: &[Statement], source: &Place, local: Local) -> bool {
    statements.iter().any(|statement| {
        matches!(&statement.kind,
            StatementKind::IncRef(place) if place == source || *place == Place::new(local))
    })
}
