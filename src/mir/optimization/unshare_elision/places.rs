// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The places the analysis may come to know unshared, numbered densely.
//!
//! Only two things put a place into the facts: a check answering no about
//! it, and a store giving it a value only it holds. Every other place of the
//! body is never known unshared, so the analysis tracks just these, each
//! under a [`FactId`], and the places of one local under consecutive ids.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::facts::{FactId, Facts};
use super::{fresh_source, hands_back_unshared, FreshSource};
use crate::mir::{Body, Local, Place, PlaceElem, Rvalue, StatementKind, TerminatorKind};

/// What a tracked place is reached through.
#[derive(Debug, Clone, Copy)]
enum Parent {
    /// Nothing: the place is a whole local.
    Root,
    /// A place the table tracks too.
    Tracked(FactId),
    /// A place never known unshared, so neither is this one.
    Untracked,
}

/// The tracked places and how each relates to the others.
pub(super) struct PlaceTable {
    ids: HashMap<Place, FactId>,
    places: Vec<Place>,
    parents: Vec<Parent>,
    /// The ids of the places rooted at each local, by local.
    by_root: Vec<Option<Range<usize>>>,
    /// The ids of the places indexed by each local, by local.
    by_index: Vec<Vec<FactId>>,
}

impl PlaceTable {
    /// Tracks every place in `body` that may come to be known unshared and
    /// that no fact is refused for (see [`Eligibility`]).
    pub(super) fn of(body: &Body, asked: &[&Place]) -> Self {
        let eligibility = Eligibility::of(body);
        let mut seen = HashSet::new();
        let mut places: Vec<Place> = candidates(body, asked)
            .filter(|place| eligibility.is_trackable(place) && seen.insert(*place))
            .cloned()
            .collect();
        places.sort_by_key(|place| place.local.0);
        Self::numbering(places, body.local_decls.len())
    }

    fn numbering(places: Vec<Place>, locals: usize) -> Self {
        let ids: HashMap<Place, FactId> = places
            .iter()
            .enumerate()
            .map(|(index, place)| (place.clone(), FactId(index)))
            .collect();
        let parents = places.iter().map(|place| parent_of(place, &ids)).collect();
        let mut by_root: Vec<Option<Range<usize>>> = vec![None; locals];
        let mut by_index = vec![Vec::new(); locals];
        for (index, place) in places.iter().enumerate() {
            if let Some(range) = by_root.get_mut(place.local.0) {
                let start = range.as_ref().map_or(index, |range| range.start);
                *range = Some(start..index + 1);
            }
            for local in place.index_locals() {
                if let Some(indexed) = by_index.get_mut(local.0) {
                    indexed.push(FactId(index));
                }
            }
        }
        Self {
            ids,
            places,
            parents,
            by_root,
            by_index,
        }
    }

    /// How many places are tracked.
    pub(super) fn len(&self) -> usize {
        self.places.len()
    }

    /// The id of `place`; `None` when it is never known unshared.
    pub(super) fn id(&self, place: &Place) -> Option<FactId> {
        self.ids.get(place).copied()
    }

    /// Records the place `id` as unshared, when every place it is reached
    /// through is already known unshared.
    pub(super) fn record_unshared(&self, facts: &mut Facts, id: FactId) {
        if self.is_reached_through_known(facts, id) {
            facts.insert(id);
        }
    }

    fn is_reached_through_known(&self, facts: &Facts, id: FactId) -> bool {
        let mut current = id;
        loop {
            match self.parents.get(current.0) {
                Some(Parent::Root) => return true,
                Some(Parent::Tracked(parent)) if facts.contains(*parent) => current = *parent,
                Some(Parent::Tracked(_) | Parent::Untracked) | None => return false,
            }
        }
    }

    /// Every tracked place rooted at `local`, with its id.
    pub(super) fn rooted_at(&self, local: Local) -> impl Iterator<Item = (FactId, &Place)> {
        let range = self.by_root.get(local.0).cloned().flatten().unwrap_or(0..0);
        let places = self.places.get(range.clone()).unwrap_or_default();
        range.map(FactId).zip(places)
    }

    /// Every tracked place a read or write of `reach` reaches
    /// ([`Place::covers`]), with its id.
    pub(super) fn covered_by<'t>(
        &'t self,
        reach: &'t Place,
    ) -> impl Iterator<Item = (FactId, &'t Place)> + 't {
        self.rooted_at(reach.local)
            .filter(move |(_, place)| reach.covers(place))
    }

    /// The ids of the tracked places indexed by `local`.
    pub(super) fn indexed_by(&self, local: Local) -> &[FactId] {
        self.by_index.get(local.0).map_or(&[], Vec::as_slice)
    }
}

fn parent_of(place: &Place, ids: &HashMap<Place, FactId>) -> Parent {
    let Some((_, outer)) = place.projection.split_last() else {
        return Parent::Root;
    };
    let prefix = Place {
        local: place.local,
        projection: outer.to_vec(),
    };
    ids.get(&prefix)
        .map_or(Parent::Untracked, |id| Parent::Tracked(*id))
}

/// Every place that may come to be known unshared: each place a check or a
/// copy-on-write call asks about, and each destination of a store that may
/// give it a value only it holds.
fn candidates<'a>(body: &'a Body, asked: &'a [&'a Place]) -> impl Iterator<Item = &'a Place> {
    let asked = asked.iter().copied();
    let stored = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                may_be_fresh(rvalue).then_some(place)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        });
    let allocated = body
        .basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                destination,
                args: _,
                arg_modes: _,
                arg_handles: _,
                target: _,
            } => hands_back_unshared(func).then_some(destination),
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => None,
        });
    asked.chain(stored).chain(allocated)
}

fn may_be_fresh(rvalue: &Rvalue) -> bool {
    match fresh_source(rvalue) {
        FreshSource::Built | FreshSource::Moved(_) => true,
        FreshSource::Neither => false,
    }
}

/// Which locals a fact may be reached through.
struct Eligibility {
    /// By local: whether a value held there may be held or changed by code
    /// this body does not show — a parameter's or a capture's by the caller, a
    /// gpu-resident binding's by the device, an address-taken local's through
    /// the pointer. No fact is rooted at one.
    foreign_root: Vec<bool>,
    /// By local: whether the local's address is taken, so a store through the
    /// pointer may change it unseen. No fact is indexed by one: the element it
    /// names could change under the fact. Any other index local changes only
    /// by a visible write, which forgets every fact indexed by it.
    unstable_index: Vec<bool>,
}

impl Eligibility {
    fn of(body: &Body) -> Self {
        let mut unstable_index = vec![false; body.local_decls.len()];
        for place in referenced_places(body) {
            for local in std::iter::once(place.local).chain(place.index_locals()) {
                mark(&mut unstable_index, local);
            }
        }
        let mut foreign_root = unstable_index.clone();
        let captured = body.env_capture_locals.iter().copied();
        let resident = (body.local_decls.iter().enumerate())
            .filter(|(_, decl)| decl.device_handle.is_some())
            .map(|(index, _)| Local(index));
        for local in (1..=body.arg_count)
            .map(Local)
            .chain(captured)
            .chain(resident)
        {
            mark(&mut foreign_root, local);
        }
        Self {
            foreign_root,
            unstable_index,
        }
    }

    /// Whether a fact may name `place`: it reaches nothing through a pointer,
    /// its local is not foreign and no local indexing it is unstable.
    fn is_trackable(&self, place: &Place) -> bool {
        let is_set = |flags: &[bool], local: Local| flags.get(local.0).copied().unwrap_or(true);
        !is_set(&self.foreign_root, place.local)
            && !place.projection.contains(&PlaceElem::Deref)
            && !place
                .index_locals()
                .any(|local| is_set(&self.unstable_index, local))
    }
}

fn mark(flags: &mut [bool], local: Local) {
    if let Some(flag) = flags.get_mut(local.0) {
        *flag = true;
    }
}

/// Every place whose address the body takes.
fn referenced_places(body: &Body) -> impl Iterator<Item = &Place> {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place))
            | StatementKind::Reassign(_, Rvalue::Ref(place)) => Some(place),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        })
}
