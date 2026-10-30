// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that a runtime entry point writes a collection in place only where
//! every path to the write made it the program's own.
//!
//! A runtime write (`runtime_fns::writes_in_place`) of a value a write copies
//! first (`type_facts::unsharing::is_copied_on_write`) lands in whatever block
//! its argument names, so every other name holding that block would see it.
//! Each such argument must therefore be known, on every path into the call,
//! to hold a block nothing else holds. The proof is re-derived here from the
//! body alone, by a forward must-dataflow of its own, without the passes that
//! place or drop the checks.
//!
//! A place becomes known on the edge a sharing check takes when the answer is
//! no, as the result of a clone or a copy-on-write entry point, when a struct
//! just built is stored into it, when a known value nothing else names is
//! moved into it, when it is moved out and straight back, and after its
//! receiver check returns. The value a map holds under a key is known after
//! that value is checked, or a known value is stored there; a later lookup
//! under a key holding the same value hands back a known place.
//!
//! A place is forgotten, with every place reached through it, when it is
//! retained, copied, moved elsewhere, stored into, released, or its storage
//! starts or ends; when an index local or a key it depends on is given another
//! value; and when it is handed to a function of the program, a virtual call
//! or a kernel. A place the program does not hold privately — one rooted at a
//! parameter, a capture, a borrow, a gpu-resident binding or a local whose
//! address is taken, or reached through a value no write copies, like a class
//! instance — is forgotten at every point that may run program code: such a
//! call, a release that may run a drop hook, and a runtime call that calls a
//! closure or may release an element whose release runs a hook.
//!
//! A runtime write keeps the place it writes known and forgets only what is
//! reached through it: the runtime keeps no reference to what it writes in
//! place (`runtime_fns::writes_in_place`, pinned by the contract test
//! `a_runtime_write_keeps_nothing_it_is_handed`), so the count it found is the
//! count it leaves. A GPU body holds no count and is skipped.

use super::{local_display_name, VerificationViolation};
use crate::mir::analysis::traversal::reverse_postorder;
use crate::mir::place::PlaceContext;
use crate::mir::place_type::PlaceTypes;
use crate::mir::residency::{READBACK_FN, UPLOAD_FN};
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::type_facts::DropHookReach;
use crate::mir::visitor::Visitor;
use crate::mir::{
    AggregateKind, BasicBlock, Body, Discriminant, Local, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::{
    allocates_collection, closure_positions, element_positions, is_copy_on_write, returns_unshared,
    rt, taken_argument_positions, writes_in_place,
};
use std::collections::{HashMap, HashSet};

/// Every runtime write in `body` of a value another name may hold.
pub fn verify_unshared_writes(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() || !body.basic_blocks.iter().any(ends_in_runtime_call) {
        return Vec::new();
    }
    let model = Model::of(body);
    let entries = model.entry_states();
    (body.basic_blocks.iter().enumerate())
        .filter_map(|(block, data)| Some((block, data, entries.get(block)?.as_ref()?)))
        .flat_map(|(block, data, entry)| model.block_violations(block, data, entry.clone()))
        .collect()
}

fn ends_in_runtime_call(data: &crate::mir::BasicBlockData) -> bool {
    matches!(
        data.terminator.as_ref().map(|terminator| &terminator.kind),
        Some(TerminatorKind::Call { func, .. }) if func.called_runtime_name().is_some()
    )
}

/// One thing known at a point of the body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Fact {
    /// The place holds a block nothing else holds.
    Unshared(Place),
    /// The value `map` holds under the value `key` holds is unshared.
    Entry { map: Place, key: Local },
    /// `copy` holds the value `source` holds.
    Same { copy: Local, source: Local },
    /// `result` holds what `map` held under `key` when it was looked up.
    Lookup {
        result: Local,
        map: Place,
        key: Local,
    },
    /// `temp` holds the known value just moved out of `owner`, and nothing
    /// has happened since.
    MovedOut { temp: Local, owner: Local },
}

type Facts = HashSet<Fact>;

/// The body and the facts about it the dataflow reads.
struct Model<'a> {
    body: &'a Body,
    places: PlaceTypes<'a>,
    hooks: DropHookReach<'a>,
    /// Locals whose value something outside the body may also reach.
    shared_roots: HashSet<Local>,
    /// Per local, how often the body names it outside storage markers.
    mentions: Vec<usize>,
    /// Per local, how many times the whole of it is given a value.
    definitions: Vec<usize>,
    /// Each local a map lookup hands back, with the maps it looks into.
    lookups: HashMap<Local, Vec<Place>>,
    /// Each block branching on a sharing check, with the checked place and
    /// the block the "no" answer goes to.
    checks: HashMap<usize, (Place, usize)>,
    predecessors: Vec<Vec<usize>>,
}

impl<'a> Model<'a> {
    fn of(body: &'a Body) -> Self {
        let mut predecessors = vec![Vec::new(); body.basic_blocks.len()];
        for (index, data) in body.basic_blocks.iter().enumerate() {
            for successor in data.terminator.iter().flat_map(Terminator::successors) {
                if let Some(list) = predecessors.get_mut(successor.0) {
                    list.push(index);
                }
            }
        }
        let (mentions, definitions) = count_uses(body);
        let mut model = Self {
            body,
            places: PlaceTypes::of(body),
            hooks: DropHookReach::new(body.rc_classifier.definitions()),
            shared_roots: shared_roots(body),
            mentions,
            definitions,
            lookups: lookups(body),
            checks: HashMap::new(),
            predecessors,
        };
        model.checks = model.sharing_checks();
        model
    }

    /// The facts known on entry to each block the entry reaches; `None` for
    /// a block it never reaches.
    fn entry_states(&self) -> Vec<Option<Facts>> {
        let count = self.body.basic_blocks.len();
        let order = reverse_postorder(self.body);
        let mut entries: Vec<Option<Facts>> = vec![None; count];
        let mut exits: Vec<Option<Facts>> = vec![None; count];
        let mut changed = true;
        while changed {
            changed = false;
            for &block in &order {
                let entry = self.meet(block, &exits);
                let exit = entry.clone().map(|facts| self.run_block(block, facts));
                changed |= entry != entries[block] || exit != exits[block];
                entries[block] = entry;
                exits[block] = exit;
            }
        }
        entries
    }

    fn meet(&self, block: usize, exits: &[Option<Facts>]) -> Option<Facts> {
        let mut result: Option<Facts> = (block == 0).then(Facts::new);
        for &pred in self.predecessors.get(block).into_iter().flatten() {
            let Some(Some(exit)) = exits.get(pred) else {
                continue;
            };
            let edge = self.along_edge(pred, block, exit);
            result = Some(match result {
                None => edge,
                Some(so_far) => so_far.intersection(&edge).cloned().collect(),
            });
        }
        result
    }

    /// `exit`, the facts leaving `from`, as they arrive at `to`: with the
    /// checked place known when `from` takes the "no" answer of a check.
    fn along_edge(&self, from: usize, to: usize, exit: &Facts) -> Facts {
        let mut facts = exit.clone();
        let Some((checked, no)) = self.checks.get(&from) else {
            return facts;
        };
        if *no != to {
            return facts;
        }
        let entries: Vec<Fact> = (exit.iter())
            .filter_map(|fact| match fact {
                Fact::Lookup { result, map, key } if Place::new(*result) == *checked => {
                    Some(entries_under(map, *key, exit))
                }
                Fact::Unshared(_)
                | Fact::Entry { .. }
                | Fact::Same { .. }
                | Fact::Lookup { .. }
                | Fact::MovedOut { .. } => None,
            })
            .flatten()
            .collect();
        facts.extend(entries);
        facts.insert(Fact::Unshared(checked.clone()));
        facts
    }

    fn run_block(&self, block: usize, mut facts: Facts) -> Facts {
        let Some(data) = self.body.basic_blocks.get(block) else {
            return facts;
        };
        for statement in &data.statements {
            self.step(statement, &mut facts);
        }
        if let Some(terminator) = &data.terminator {
            self.step_terminator(terminator, &mut facts);
        }
        facts
    }

    /// Every runtime write `data`, at `block`, makes of a value not known
    /// unshared there, given the facts on entry.
    fn block_violations(
        &self,
        block: usize,
        data: &crate::mir::BasicBlockData,
        mut facts: Facts,
    ) -> Vec<VerificationViolation> {
        let Some(TerminatorKind::Call { func, args, .. }) =
            data.terminator.as_ref().map(|terminator| &terminator.kind)
        else {
            return Vec::new();
        };
        let Some(name) = func.called_runtime_name() else {
            return Vec::new();
        };
        for statement in &data.statements {
            self.step(statement, &mut facts);
        }
        (args.iter().enumerate())
            .filter(|(position, _)| writes_in_place(name, *position))
            .filter_map(|(_, arg)| arg.place())
            .filter(|place| self.is_checked_write(name, place))
            .filter(|place| !facts.contains(&Fact::Unshared((*place).clone())))
            .map(|place| VerificationViolation {
                local: place.local,
                local_name: local_display_name(self.body, place.local),
                message: format!(
                    "bb{block} writes `{place}` in place through `{name}` while another \
                     name may hold it"
                ),
            })
            .collect()
    }

    /// Whether a write of `place` by the runtime entry point `name` must
    /// find it unshared: it holds a value a write copies first, or one whose
    /// type the body cannot read.
    ///
    /// A gpu-resident binding is exempt, and so is the host side of the
    /// transfers residency emits: the host block of a binding mirrors a
    /// device buffer, every write's copy-on-write leaves it alone, and an
    /// upload or a readback hands its host array to the runtime unchecked,
    /// for a binding or for a buffer lowering built for one (the array a
    /// whole-array assignment uploads, a reduction's result buffer).
    // TODO: an upload only reads its host array and belongs in
    // `runtime_fns::only_reads_argument`; a readback writes its host array
    // in place, and whether a readback into a gpu-resident parameter's host
    // array leaves the caller's value as it was is undecided. Until both are
    // settled these arguments are not checked.
    fn is_checked_write(&self, name: &str, place: &Place) -> bool {
        let is_device_resident = (self.body.local_decls.get(place.local.0))
            .is_some_and(|decl| decl.device_handle.is_some());
        let is_transfer = name == READBACK_FN || name == UPLOAD_FN;
        // TODO: a vector component place (`v.x`) has no declared type yet, so
        // a component handed to a runtime call types as unknown and is checked
        // as if it were a collection; `FLOAT_TO_STRING` and `F32_TO_STRING` are
        // listed as read-only to cover the calls that hand one today.
        !is_device_resident
            && !is_transfer
            && self
                .places
                .declared(place)
                .is_none_or(|ty| is_copied_on_write(self.body, &ty))
    }

    fn step(&self, statement: &Statement, facts: &mut Facts) {
        match &statement.kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                self.step_assign(dest, rvalue, facts)
            }
            StatementKind::IncRef(place) => {
                // TODO: retaining a runtime call's result forgets only facts on
                // that result's place, not on the place it may alias (an
                // element a lookup hands back), so a stale unshared fact can
                // survive the retain; lowering checks again before every write
                // today, so no write relies on one.
                forget_moves(facts);
                self.forget_shared(place, facts);
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                forget_moves(facts);
                self.forget(place, facts);
                if self.may_run_hook(place) {
                    self.forget_non_private(facts);
                }
            }
            StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                forget_moves(facts);
                forget_local(place.local, facts);
            }
            StatementKind::Nop => {}
        }
    }

    fn step_assign(&self, dest: &Place, rvalue: &Rvalue, facts: &mut Facts) {
        let made = self.assigned_facts(dest, rvalue, facts);
        forget_moves(facts);
        for read in read_places(rvalue) {
            self.forget_shared(read, facts);
        }
        self.forget_store(dest, facts);
        facts.extend(made);
    }

    /// What storing `rvalue` into `dest` makes known, given `facts` before.
    fn assigned_facts(&self, dest: &Place, rvalue: &Rvalue, facts: &Facts) -> Vec<Fact> {
        let whole_dest = dest.projection.is_empty().then_some(dest.local);
        match rvalue {
            Rvalue::Aggregate(kind, operands) if is_fresh_aggregate(kind, operands) => {
                vec![Fact::Unshared(dest.clone())]
            }
            Rvalue::Use(Operand::Move(source)) if source.projection.is_empty() => {
                let mut made = self.moved_facts(dest, source.local, facts);
                if let Some(copy) = whole_dest {
                    made.extend(same(copy, source.local, facts));
                }
                made
            }
            Rvalue::Use(Operand::Copy(source)) if source.projection.is_empty() => whole_dest
                .map(|copy| same(copy, source.local, facts))
                .unwrap_or_default(),
            Rvalue::Use(_)
            | Rvalue::Aggregate(..)
            | Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => Vec::new(),
        }
    }

    /// What moving the whole of `source` into `dest` makes known: `dest`,
    /// when `source` is known and named nowhere else or is the value `dest`
    /// handed it the step before; a pending round trip, when the known value
    /// of `source` moves into a whole local.
    fn moved_facts(&self, dest: &Place, source: Local, facts: &Facts) -> Vec<Fact> {
        let whole_dest = dest.projection.is_empty().then_some(dest.local);
        let returns = whole_dest.is_some_and(|owner| {
            facts.contains(&Fact::MovedOut {
                temp: source,
                owner,
            })
        });
        let is_known = facts.contains(&Fact::Unshared(Place::new(source)));
        if returns || (is_known && self.is_named_once(source)) {
            return vec![Fact::Unshared(dest.clone())];
        }
        (whole_dest.filter(|_| is_known))
            .map(|temp| Fact::MovedOut {
                temp,
                owner: source,
            })
            .into_iter()
            .collect()
    }

    /// Whether `local` is given a value once and read once, so a move out
    /// of it leaves no other name of its value.
    fn is_named_once(&self, local: Local) -> bool {
        self.definitions.get(local.0) == Some(&1) && self.mentions.get(local.0) == Some(&2)
    }

    fn step_terminator(&self, terminator: &Terminator, facts: &mut Facts) {
        match &terminator.kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => return,
            TerminatorKind::Call { func, args, .. } => {
                forget_moves(facts);
                match func.called_runtime_name() {
                    Some(name) => return self.step_runtime_call(name, terminator, args, facts),
                    None => self.forget_handed(args.iter().chain([func]), facts),
                }
            }
            TerminatorKind::VirtualCall { args, .. } => {
                forget_moves(facts);
                self.forget_handed(args.iter(), facts);
            }
            TerminatorKind::GpuLaunch {
                launch_args,
                scalar_args,
                ..
            } => {
                forget_moves(facts);
                self.forget_handed(launch_args.args().iter().chain(scalar_args), facts);
            }
        }
        for written in terminator.written_places() {
            self.forget_store(written, facts);
        }
    }

    fn step_runtime_call(
        &self,
        name: &str,
        terminator: &Terminator,
        args: &[Operand],
        facts: &mut Facts,
    ) {
        let TerminatorKind::Call { destination, .. } = &terminator.kind else {
            return;
        };
        let made = self.runtime_facts(name, args, destination, facts);
        let mut runs_program = !closure_positions(name).is_empty();
        for (position, arg) in args.iter().enumerate() {
            let Some(place) = arg.place() else {
                continue;
            };
            let is_kept = taken_argument_positions(name).contains(&position)
                || element_positions(name).contains(&position);
            if is_kept {
                self.forget(place, facts);
            } else if writes_in_place(name, position) {
                self.forget_within(place, facts);
                runs_program |= self.may_run_hook(place);
            }
        }
        if runs_program {
            self.forget_non_private(facts);
        }
        for written in terminator.written_places() {
            self.forget_store(written, facts);
        }
        facts.extend(made);
    }

    /// What a call to the runtime entry point `name` makes known once it
    /// returns, given `facts` before it.
    fn runtime_facts(
        &self,
        name: &str,
        args: &[Operand],
        destination: &Place,
        facts: &Facts,
    ) -> Vec<Fact> {
        let place_at = |position: usize| args.get(position).and_then(Operand::place);
        let key_at = |position: usize| args.get(position).and_then(Operand::bare_local);
        if is_copy_on_write(name) || returns_unshared(name) || allocates_collection(name) {
            return vec![Fact::Unshared(destination.clone())];
        }
        if name == rt::RECEIVER_UNIQUE {
            return place_at(0)
                .cloned()
                .map(Fact::Unshared)
                .into_iter()
                .collect();
        }
        let map_and_key = place_at(0).zip(key_at(1));
        if name == rt::MAP_GET_CHECKED {
            return map_and_key
                .map(|(map, key)| looked_up(destination, map, key, facts))
                .unwrap_or_default();
        }
        let is_stored_unshared = name == rt::MAP_SET
            && place_at(2).is_some_and(|value| facts.contains(&Fact::Unshared(value.clone())));
        map_and_key
            .filter(|_| is_stored_unshared)
            .map(|(map, key)| entries_under(map, key, facts))
            .unwrap_or_default()
    }

    /// Forgets every place `operands` hand a call that may run program code,
    /// with the places they borrow from or lend to, and every fact on a
    /// place the program does not hold privately.
    fn forget_handed<'o>(&self, operands: impl Iterator<Item = &'o Operand>, facts: &mut Facts) {
        for place in operands.filter_map(Operand::place) {
            self.forget(place, facts);
            let lends_to = (self.body.borrowed_locals.iter())
                .filter(|(_, owners)| owners.contains(&place.local))
                .map(|(borrow, _)| borrow);
            let borrows_from = self.body.borrowed_locals.get(&place.local);
            for local in lends_to.chain(borrows_from.into_iter().flatten()) {
                self.forget(&Place::new(*local), facts);
            }
        }
        self.forget_non_private(facts);
    }

    /// Forgets `place` and everything reached through it: it is now shared,
    /// or held elsewhere. A lookup's result names a map's entry, so sharing
    /// it shares that entry.
    fn forget(&self, place: &Place, facts: &mut Facts) {
        facts.retain(|fact| match fact {
            Fact::Unshared(known) => !place.covers(known),
            Fact::Entry { map, .. } => !place.covers(map),
            Fact::Lookup { result, map, .. } => !place.covers(map) && *result != place.local,
            Fact::Same { .. } | Fact::MovedOut { .. } => true,
        });
        for map in self.lookups.get(&place.local).into_iter().flatten() {
            self.forget_entries(map, facts);
        }
    }

    /// Forgets what a second holder of `place`'s value now reaches: `place`
    /// itself, and each place reached through it by values a write copies.
    /// A class instance is shared by design, so another name of one does
    /// not hold the blocks its fields hold a second time; a fact below it
    /// lives on until code that may reach the instance runs.
    fn forget_shared(&self, place: &Place, facts: &mut Facts) {
        facts.retain(|fact| match fact {
            Fact::Unshared(known) => !self.is_reached_by_copies(place, known),
            Fact::Entry { map, .. } => !self.is_reached_by_copies(place, map),
            Fact::Lookup { result, map, .. } => {
                !self.is_reached_by_copies(place, map) && *result != place.local
            }
            Fact::Same { .. } | Fact::MovedOut { .. } => true,
        });
        for map in self.lookups.get(&place.local).into_iter().flatten() {
            self.forget_entries(map, facts);
        }
    }

    /// Whether `place` covers `known` through values a write copies alone:
    /// `place` and every place between it and `known`.
    fn is_reached_by_copies(&self, place: &Place, known: &Place) -> bool {
        place.covers(known)
            && (place.projection.len()..known.projection.len()).all(|length| {
                let between = Place {
                    local: known.local,
                    projection: known.projection[..length].to_vec(),
                };
                (self.places.declared(&between)).is_none_or(|ty| is_copied_on_write(self.body, &ty))
            })
    }

    /// Forgets what is reached through `place`, which a runtime entry point
    /// writes in place, keeping `place` itself: its entries and elements
    /// may move, be released or be replaced.
    fn forget_within(&self, place: &Place, facts: &mut Facts) {
        facts.retain(|fact| match fact {
            Fact::Unshared(known) => {
                !place.covers(known) || known.projection.len() <= place.projection.len()
            }
            Fact::Entry { .. }
            | Fact::Same { .. }
            | Fact::Lookup { .. }
            | Fact::MovedOut { .. } => true,
        });
        self.forget_entries(place, facts);
    }

    /// Forgets every entry of `map` and every lookup into it.
    fn forget_entries(&self, map: &Place, facts: &mut Facts) {
        let results: Vec<Local> = (self.lookups.iter())
            .filter(|(_, maps)| maps.iter().any(|looked| map.covers(looked)))
            .map(|(result, _)| *result)
            .collect();
        facts.retain(|fact| match fact {
            Fact::Unshared(known) => !results.contains(&known.local),
            Fact::Entry { map: entry_map, .. } | Fact::Lookup { map: entry_map, .. } => {
                !map.covers(entry_map)
            }
            Fact::Same { .. } | Fact::MovedOut { .. } => true,
        });
    }

    /// Forgets what a store into `place` replaces. A store through a
    /// dereference may land in any place whose address is taken.
    fn forget_store(&self, place: &Place, facts: &mut Facts) {
        if place.projection.is_empty() {
            forget_local(place.local, facts);
            return;
        }
        self.forget(place, facts);
        if place.projection.contains(&PlaceElem::Deref) {
            self.forget_non_private(facts);
        }
    }

    /// Forgets every fact on a place something outside the body may reach,
    /// before code of the program that may reach it runs.
    fn forget_non_private(&self, facts: &mut Facts) {
        let before = facts.clone();
        facts.retain(|fact| match fact {
            Fact::Unshared(place) => self.is_private(place, &before),
            Fact::Entry { map, .. } => self.is_private(map, &before),
            Fact::Lookup { result, map, .. } => {
                self.is_private(map, &before) && !self.shared_roots.contains(result)
            }
            Fact::Same { copy, source } => {
                !self.shared_roots.contains(copy) && !self.shared_roots.contains(source)
            }
            Fact::MovedOut { temp, owner } => {
                !self.shared_roots.contains(temp) && !self.shared_roots.contains(owner)
            }
        });
    }

    /// Whether only this body reaches `place`: its root is a local of its
    /// own nothing else reaches, and every value it is reached through is
    /// one a write copies first or, like a class instance just built, one
    /// `facts` know no other name holds.
    fn is_private(&self, place: &Place, facts: &Facts) -> bool {
        if self.shared_roots.contains(&place.local) {
            return false;
        }
        (0..place.projection.len()).all(|length| {
            let prefix = Place {
                local: place.local,
                projection: place.projection[..length].to_vec(),
            };
            let is_copied = (self.places.declared(&prefix))
                .is_some_and(|ty| is_copied_on_write(self.body, &ty));
            is_copied || facts.contains(&Fact::Unshared(prefix))
        })
    }

    fn may_run_hook(&self, place: &Place) -> bool {
        self.places
            .declared(place)
            .is_none_or(|ty| self.hooks.may_run_drop_hook(&ty))
    }

    /// Each block that branches on a sharing check's answer, with the place
    /// checked and the block the "no" answer goes to: the check's own
    /// successor, entered from it alone, doing nothing but branching on an
    /// answer nothing else gives a value, to one block for 0 and another
    /// otherwise.
    fn sharing_checks(&self) -> HashMap<usize, (Place, usize)> {
        let mut checks = HashMap::new();
        for (call, data) in self.body.basic_blocks.iter().enumerate() {
            let Some(TerminatorKind::Call {
                func,
                args,
                destination,
                target: Some(switch),
                ..
            }) = data.terminator.as_ref().map(|terminator| &terminator.kind)
            else {
                continue;
            };
            let [arg] = args.as_slice() else {
                continue;
            };
            let is_check = func.called_runtime_name() == Some(rt::RC_IS_SHARED)
                && destination.projection.is_empty()
                && self.definitions.get(destination.local.0) == Some(&1)
                && self.predecessors.get(switch.0).map(Vec::as_slice) == Some(&[call][..]);
            let no = (self.body.basic_blocks.get(switch.0))
                .filter(|block| is_check && block.statements.is_empty())
                .and_then(|block| block.terminator.as_ref())
                .and_then(|terminator| no_target(terminator, destination.local));
            if let Some((place, no)) = arg.place().zip(no) {
                checks.insert(switch.0, (place.clone(), no.0));
            }
        }
        checks
    }
}

/// Whether an aggregate of `kind` built from `operands` is a block only its
/// destination holds: a struct or a class instance just built, or an empty
/// map or set, which code generation allocates afresh (`miri_rt_map_new`,
/// `miri_rt_set_new`). An array may be an immortal constant, a tuple is not
/// known to be allocated afresh, and an option, enum or closure holds its
/// operand itself.
fn is_fresh_aggregate(kind: &AggregateKind, operands: &[Operand]) -> bool {
    match kind {
        AggregateKind::Struct(_) | AggregateKind::Class(_) => true,
        AggregateKind::Map | AggregateKind::Set => operands.is_empty(),
        AggregateKind::Tuple(_)
        | AggregateKind::Array
        | AggregateKind::List
        | AggregateKind::FormattedString
        | AggregateKind::Enum(..)
        | AggregateKind::Option(_)
        | AggregateKind::Closure(..) => false,
    }
}

/// The block `terminator` goes to when `answer` is 0, when it branches on
/// `answer` alone to one block for 0 and a different one otherwise.
fn no_target(terminator: &Terminator, answer: Local) -> Option<BasicBlock> {
    let TerminatorKind::SwitchInt {
        discr,
        targets,
        otherwise,
    } = &terminator.kind
    else {
        return None;
    };
    let [(value, no)] = targets.as_slice() else {
        return None;
    };
    let is_answer = discr.bare_local() == Some(answer);
    (is_answer && *value == Discriminant::from(0) && no != otherwise).then_some(*no)
}

/// What a lookup of `key` in `map` into `destination` makes known: the
/// lookup itself, and its result when the entry under that key is known.
fn looked_up(destination: &Place, map: &Place, key: Local, facts: &Facts) -> Vec<Fact> {
    if !destination.projection.is_empty() {
        return Vec::new();
    }
    let result = destination.local;
    let is_entry_known = key_names(key, facts).into_iter().any(|name| {
        facts.contains(&Fact::Entry {
            map: map.clone(),
            key: name,
        })
    });
    let lookup = Fact::Lookup {
        result,
        map: map.clone(),
        key,
    };
    let known = is_entry_known.then(|| Fact::Unshared(destination.clone()));
    [lookup].into_iter().chain(known).collect()
}

/// The entry of `map` under `key` known unshared, under every local
/// `facts` know holds the key's value, so two paths that spell one key
/// differently still agree where they meet.
fn entries_under(map: &Place, key: Local, facts: &Facts) -> Vec<Fact> {
    key_names(key, facts)
        .into_iter()
        .map(|name| Fact::Entry {
            map: map.clone(),
            key: name,
        })
        .collect()
}

/// The facts that `copy` now holds the value `source` holds, and the value
/// of every local `source` is known to hold a copy of.
fn same(copy: Local, source: Local, facts: &Facts) -> Vec<Fact> {
    let mut made = vec![Fact::Same { copy, source }];
    made.extend(sources_of(source, facts).map(|source| Fact::Same { copy, source }));
    made
}

fn sources_of(copy: Local, facts: &Facts) -> impl Iterator<Item = Local> + '_ {
    facts.iter().filter_map(move |fact| match fact {
        Fact::Same {
            copy: named,
            source,
        } if *named == copy => Some(*source),
        Fact::Unshared(_)
        | Fact::Entry { .. }
        | Fact::Same { .. }
        | Fact::Lookup { .. }
        | Fact::MovedOut { .. } => None,
    })
}

/// Every local `facts` know holds the value `key` holds: `key`, the locals
/// it copies, and the other copies of those.
fn key_names(key: Local, facts: &Facts) -> Vec<Local> {
    let mut roots: Vec<Local> = sources_of(key, facts).collect();
    roots.push(key);
    let copies: Vec<Local> = (facts.iter())
        .filter_map(|fact| match fact {
            Fact::Same { copy, source } if roots.contains(source) => Some(*copy),
            Fact::Unshared(_)
            | Fact::Entry { .. }
            | Fact::Same { .. }
            | Fact::Lookup { .. }
            | Fact::MovedOut { .. } => None,
        })
        .collect();
    roots.extend(copies);
    roots
}

/// Forgets the pending round trips: something happened since the move out.
fn forget_moves(facts: &mut Facts) {
    facts.retain(|fact| !matches!(fact, Fact::MovedOut { .. }));
}

/// Forgets every fact that depends on the value the whole of `local` holds.
fn forget_local(local: Local, facts: &mut Facts) {
    let names = |place: &Place| place.local == local || place.index_locals().any(|i| i == local);
    facts.retain(|fact| match fact {
        Fact::Unshared(place) => !names(place),
        Fact::Entry { map, key } => !names(map) && *key != local,
        Fact::Same { copy, source } => *copy != local && *source != local,
        Fact::Lookup { result, map, key } => *result != local && !names(map) && *key != local,
        Fact::MovedOut { temp, owner } => *temp != local && *owner != local,
    });
}

/// The places `rvalue` reads a value out of, each of which another place
/// may hold afterwards. A length, an arithmetic result and a comparison
/// hold nothing of what they read.
fn read_places(rvalue: &Rvalue) -> Vec<&Place> {
    let operands: Vec<&Operand> = match rvalue {
        Rvalue::Use(operand) => vec![operand],
        Rvalue::Cast(operand, _) => vec![operand.as_ref()],
        Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) => {
            operands.iter().collect()
        }
        Rvalue::Phi(arms) => arms.iter().map(|(operand, _)| operand).collect(),
        Rvalue::AtomicOp {
            buffer,
            value,
            compare_expected,
            ..
        } => [buffer.as_ref(), value.as_ref()]
            .into_iter()
            .chain(compare_expected.as_deref())
            .collect(),
        Rvalue::Ref(place) => return vec![place],
        Rvalue::Len(_) | Rvalue::BinaryOp(..) | Rvalue::UnaryOp(..) | Rvalue::GpuIntrinsic(_) => {
            Vec::new()
        }
    };
    operands.into_iter().filter_map(Operand::place).collect()
}

/// The locals whose value something outside the body may also reach:
/// parameters, captures, borrows, gpu-resident bindings and locals whose
/// address is taken.
fn shared_roots(body: &Body) -> HashSet<Local> {
    let mut roots: HashSet<Local> = (1..=body.arg_count).map(Local).collect();
    roots.extend(body.env_capture_locals.iter().copied());
    roots.extend(body.borrowed_locals.keys().copied());
    roots.extend(
        (body.local_decls.iter().enumerate())
            .filter(|(_, decl)| decl.device_handle.is_some())
            .map(|(index, _)| Local(index)),
    );
    let address_taken = (body.basic_blocks.iter())
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
        });
    roots.extend(address_taken);
    roots
}

/// Each local a map lookup hands back, with every map it is looked up in.
fn lookups(body: &Body) -> HashMap<Local, Vec<Place>> {
    let mut found: HashMap<Local, Vec<Place>> = HashMap::new();
    let calls = (body.basic_blocks.iter())
        .filter_map(|data| data.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } if func.called_runtime_name() == Some(rt::MAP_GET_CHECKED) => {
                Some((destination, args.first()?.place()?))
            }
            TerminatorKind::Call { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => None,
        });
    for (destination, map) in calls {
        found
            .entry(destination.local)
            .or_default()
            .push(map.clone());
    }
    found
}

/// Per local, how often the body names it outside storage markers, and how
/// many times the whole of it is given a value.
fn count_uses(body: &Body) -> (Vec<usize>, Vec<usize>) {
    let mut counter = Mentions {
        counts: vec![0; body.local_decls.len()],
    };
    counter.visit_body(body);
    let mut definitions = vec![0; body.local_decls.len()];
    let statement_writes = (body.basic_blocks.iter())
        .flat_map(|data| &data.statements)
        .filter_map(Statement::written_place);
    let terminator_writes = (body.basic_blocks.iter())
        .filter_map(|data| data.terminator.as_ref())
        .flat_map(Terminator::written_places);
    for place in statement_writes.chain(terminator_writes) {
        if let Some(count) = definitions
            .get_mut(place.local.0)
            .filter(|_| place.projection.is_empty())
        {
            *count += 1;
        }
    }
    (counter.counts, definitions)
}

struct Mentions {
    counts: Vec<usize>,
}

impl Visitor for Mentions {
    fn visit_local(&mut self, local: Local, context: PlaceContext, _location: BasicBlock) {
        let is_storage = matches!(
            context,
            PlaceContext::StorageLive | PlaceContext::StorageDead
        );
        if let Some(count) = self.counts.get_mut(local.0).filter(|_| !is_storage) {
            *count += 1;
        }
    }
}
