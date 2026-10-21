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
//! points that leave headers unchanged, no store through a dereference, no
//! release that may run a drop hook, and — for a parameter the body borrows —
//! no release, other than through the parameter, of a value that may hold a
//! collection of its type. Each record is re-derived here from
//! the body alone, without the pass's own reasoning, and every way it fails
//! is reported.

use super::VerificationViolation;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::instantiation::shape::constructor_parts;
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue, StableCollection, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::leaves_collection_headers_unchanged;

/// Every way a record on [`Body::stable_collections`] fails to keep its
/// collection's header.
pub fn verify_stable_collections(body: &Body) -> Vec<VerificationViolation> {
    if body.stable_collections.is_empty() {
        return Vec::new();
    }
    let checker = RecordCheck::of(body);
    body.stable_collections
        .iter()
        .flat_map(|record| checker.failures(record))
        .map(|(local, message)| VerificationViolation {
            local,
            local_name: display_name(body, local),
            message,
        })
        .collect()
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
    places: PlaceTypes<'a>,
    hooks: DropHookReach<'a>,
    predecessors: Vec<Vec<usize>>,
}

impl<'a> RecordCheck<'a> {
    fn of(body: &'a Body) -> Self {
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
            places: PlaceTypes::of(body),
            hooks: DropHookReach::new(body.rc_classifier.definitions()),
            predecessors,
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
                | StatementKind::Nop => false,
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
                if !data.statements.is_empty() || !jumps_to_header {
                    found.push(format!(
                        "its preheader {} is not a bare jump to the header",
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
            Some(terminator) => found.extend(terminator_failure(record.local, block, terminator)),
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
            StatementKind::IncRef(_) | StatementKind::Nop => None,
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
            (1..=self.body.arg_count).contains(&local.0) && !self.body.is_owned_param(local);
        let target = self.body.local_decls.get(local.0).map(|decl| &decl.ty)?;
        let released = match &statement.kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
                Some(place).filter(|place| !place.projection.is_empty())
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => Some(place),
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::Nop => None,
        }?;
        let may_free = is_borrowed
            && released.local != local
            && self
                .places
                .declared(released)
                .is_none_or(|ty| holds_collection_of(&ty, target, 0));
        may_free.then(|| format!("{block} may release the list it borrows"))
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

/// Whether a value of `ty` is, or may contain, a value of `target`'s type;
/// a declared type, a type parameter or a function may contain anything.
fn holds_collection_of(ty: &Type, target: &Type, depth: usize) -> bool {
    let declared =
        matches!(ty.kind, TypeKind::Custom(..)) && ty.kind.as_builtin_collection().is_none();
    if depth > 16 || declared || matches!(ty.kind, TypeKind::Generic(..) | TypeKind::Function(_)) {
        return true;
    }
    let (name, parts) = constructor_parts(ty);
    let (target_name, target_parts) = constructor_parts(target);
    let is_target = name == target_name
        && parts.len() == target_parts.len()
        && parts
            .iter()
            .zip(&target_parts)
            .all(|(part, target_part)| spells_same(part, target_part));
    is_target
        || parts
            .iter()
            .any(|part| holds_collection_of(part, target, depth + 1))
}

/// Whether `a` and `b` are spelled with the same constructors throughout.
fn spells_same(a: &Type, b: &Type) -> bool {
    let (a_name, a_parts) = constructor_parts(a);
    let (b_name, b_parts) = constructor_parts(b);
    a_name == b_name
        && a_parts.len() == b_parts.len()
        && a_parts.iter().zip(&b_parts).all(|(a, b)| spells_same(a, b))
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
        | StatementKind::Nop => None,
    }
}

/// What `terminator`, ending `block`, does that may change the header of
/// `local`'s collection.
fn terminator_failure(local: Local, block: usize, terminator: &Terminator) -> Option<String> {
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
        TerminatorKind::Call { func, .. } => {
            let name = func.called_runtime_name();
            (!name.is_some_and(leaves_collection_headers_unchanged)).then(|| {
                format!(
                    "bb{block} calls `{}`, which may change a header",
                    name.map_or_else(|| "a function of the program".to_string(), str::to_string)
                )
            })
        }
        TerminatorKind::VirtualCall { .. } => Some(format!(
            "bb{block} calls through a vtable, which may change a header"
        )),
        TerminatorKind::GpuLaunch { .. } => Some(format!(
            "bb{block} launches a kernel, which may change a header"
        )),
    }
}
