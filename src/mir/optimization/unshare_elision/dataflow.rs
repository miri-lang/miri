// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The forward must-analysis: which places are known unshared on entry to
//! each block, solved over a worklist taken in reverse postorder.

use std::collections::BTreeSet;

use super::effects::Program;
use super::facts::{FactId, Facts};
use super::places::PlaceTable;
use super::Check;
use crate::mir::analysis::traversal::reverse_postorder;
use crate::mir::{BasicBlock, Body, Place, Terminator};

/// The part a block plays in a check.
#[derive(Debug, Clone, Copy)]
enum Role {
    Plain,
    /// Ends with a check's call. `fact` is the place asked about, when
    /// tracked; `unshared` is where the answer "no" continues.
    Asks {
        fact: Option<FactId>,
        unshared: BasicBlock,
    },
    /// Branches on a check's answer.
    Branches {
        fact: Option<FactId>,
        unshared: BasicBlock,
    },
}

/// The facts on entry to each block, `None` for a block the entry never
/// reaches — which the meet treats as holding every fact.
pub(super) type EntryStates = Vec<Option<Facts>>;

pub(super) struct Dataflow<'a> {
    program: &'a Program,
    table: &'a PlaceTable,
    roles: Vec<Role>,
    successors: Vec<Vec<BasicBlock>>,
}

impl<'a> Dataflow<'a> {
    pub(super) fn new(
        body: &Body,
        program: &'a Program,
        table: &'a PlaceTable,
        checks: &[Check],
    ) -> Self {
        let mut roles = vec![Role::Plain; body.basic_blocks.len()];
        for check in checks {
            let fact = table.id(&check.place);
            let unshared = check.unshared;
            if let Some(role) = roles.get_mut(check.call) {
                *role = Role::Asks { fact, unshared };
            }
            if let Some(role) = roles.get_mut(check.switch) {
                *role = Role::Branches { fact, unshared };
            }
        }
        let successors = (body.basic_blocks.iter())
            .map(|block| {
                (block.terminator.as_ref())
                    .map(Terminator::successors)
                    .unwrap_or_default()
            })
            .collect();
        Self {
            program,
            table,
            roles,
            successors,
        }
    }

    /// The fixpoint of the entry states. A block is revisited only when its
    /// entry state changed, earliest in reverse postorder first.
    pub(super) fn solve(&self, body: &Body) -> EntryStates {
        let order = reverse_postorder(body);
        let mut position = vec![usize::MAX; self.roles.len()];
        for (rank, block) in order.iter().enumerate() {
            if let Some(slot) = position.get_mut(*block) {
                *slot = rank;
            }
        }
        let mut entry: EntryStates = vec![None; self.roles.len()];
        let mut pending = BTreeSet::new();
        if let Some(first) = entry.first_mut() {
            *first = Some(Facts::empty(self.table.len()));
            pending.insert(0);
        }
        while let Some(rank) = pending.pop_first() {
            let Some(&block) = order.get(rank) else {
                continue;
            };
            let Some(state) = entry.get(block).cloned().flatten() else {
                continue;
            };
            self.transfer(block, state, |successor, facts| {
                let Some(slot) = entry.get_mut(successor.0) else {
                    return;
                };
                if meet_into(slot, facts) {
                    if let Some(&rank) = position.get(successor.0) {
                        pending.insert(rank);
                    }
                }
            });
        }
        entry
    }

    /// Whether `place` is known unshared just before the terminator of
    /// `block`.
    pub(super) fn is_known_before(&self, block: usize, place: &Place, entry: &EntryStates) -> bool {
        let Some(fact) = self.table.id(place) else {
            return false;
        };
        let Some(mut facts) = entry.get(block).cloned().flatten() else {
            return false;
        };
        self.program.apply_statements(&mut facts, block, self.table);
        facts.contains(fact)
    }

    /// Hands `flow` the facts leaving `block` along each edge, given those on
    /// entry.
    ///
    /// A check whose place is already known unshared flows only to the path
    /// its answer takes, as it will once it is replaced by a jump there: its
    /// copy path is dead, and what that path forgets must not reach the join.
    /// Entry states only shrink, so a check once found unanswered never
    /// becomes answered again and the iteration still ends.
    fn transfer(&self, block: usize, mut facts: Facts, mut flow: impl FnMut(BasicBlock, &Facts)) {
        self.program.apply_statements(&mut facts, block, self.table);
        let role = self.roles.get(block).copied().unwrap_or(Role::Plain);
        if let Role::Asks {
            fact: Some(fact),
            unshared,
        } = role
        {
            if facts.contains(fact) {
                flow(unshared, &facts);
                return;
            }
        }
        self.program.apply_terminator(&mut facts, block, self.table);
        let successors = self.successors.get(block).map_or(&[][..], Vec::as_slice);
        let Role::Branches { fact, unshared } = role else {
            successors
                .iter()
                .for_each(|successor| flow(*successor, &facts));
            return;
        };
        for successor in successors
            .iter()
            .filter(|successor| **successor != unshared)
        {
            flow(*successor, &facts);
        }
        if let Some(fact) = fact {
            self.table.record_unshared(&mut facts, fact);
        }
        flow(unshared, &facts);
    }
}

/// Intersects `facts` into the state on entry to a block, treating an unset
/// state as holding everything. Returns true if the state changed.
fn meet_into(state: &mut Option<Facts>, facts: &Facts) -> bool {
    match state {
        None => {
            *state = Some(facts.clone());
            true
        }
        Some(known) => known.meet(facts),
    }
}
