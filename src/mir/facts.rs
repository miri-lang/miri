// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What passes record about a body, kept beside it rather than in it.
//!
//! A body holds the IR. What a pass works out about that IR and a later pass
//! reads lives here, grouped by the question it answers, and each group
//! remembers the body generation ([`Body::generation`]) it was recorded at.
//! A pass that rewrites the control-flow graph either declares that a group
//! still holds afterwards, and the group is carried to the new generation, or
//! leaves it stale; reading a stale group is a fault of the compiler
//! ([`StaleFact`]), never a silent use of an answer about an older body.
//!
//! [`crate::mir::passes`] does the carrying: each pass declares which groups
//! it requires, records and preserves.

use std::collections::HashMap;
use std::fmt;

use crate::mir::{Body, Local};

/// A group of facts a pass can require, record or preserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fact {
    /// [`OwnershipFacts`].
    Ownership,
}

impl Fact {
    /// Every group, in the order a body's facts hold them.
    pub const ALL: [Fact; 1] = [Fact::Ownership];
}

impl fmt::Display for Fact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fact::Ownership => f.write_str("ownership"),
        }
    }
}

/// Who holds a reference to what a body's parameters and locals hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnershipFacts {
    /// Which parameters (indexed 1..=arg_count) the body takes over from its
    /// callers. `owned_params[i-1] == true` means parameter `_i` is passed
    /// [`crate::mir::ArgMode::Owned`] by every call: the caller funds one
    /// reference for it and this body releases it. Empty means every
    /// parameter is borrowed. Decided by `crate::mir::optimization::owned_params`.
    pub owned_params: Vec<bool>,
    /// The locals that borrow a value from a place the body owns, each with
    /// every local whose value keeps what it holds alive: the roots its
    /// definitions read from and, for a borrow read out of another borrowed
    /// local, that local's owners in turn. A borrowed local holds no reference
    /// of its own, so it is neither retained when it reads nor released when
    /// its storage ends. Decided by Perceus
    /// (`crate::mir::optimization::local_borrows`); the verifier checks each
    /// entry again against the body.
    pub borrowed_locals: HashMap<Local, Vec<Local>>,
}

impl OwnershipFacts {
    /// Whether `local` is a parameter the body takes over from its callers
    /// (see [`OwnershipFacts::owned_params`]).
    pub fn is_owned_param(&self, local: Local) -> bool {
        (local.0.checked_sub(1))
            .and_then(|index| self.owned_params.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Whether `local` borrows its value from a place the body owns.
    pub fn is_borrowed(&self, local: Local) -> bool {
        self.borrowed_locals.contains_key(&local)
    }
}

/// A group was read at a body generation it was not recorded at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleFact {
    pub fact: Fact,
    /// The generation the group was last recorded or carried at, if any.
    pub recorded_at: Option<u64>,
    /// The generation of the body it was read against.
    pub read_at: u64,
}

impl fmt::Display for StaleFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.recorded_at {
            Some(at) => write!(
                f,
                "the {} facts were recorded at body generation {at} and read at {}",
                self.fact, self.read_at
            ),
            None => write!(
                f,
                "the {} facts were invalidated by a rewrite and read at body generation {}",
                self.fact, self.read_at
            ),
        }
    }
}

/// Every fact group recorded about one body, each with the generation it
/// holds at.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyFacts {
    ownership: OwnershipFacts,
    /// The generation the ownership facts hold at; `None` once a rewrite
    /// left it stale.
    ownership_at: Option<u64>,
}

impl BodyFacts {
    /// The facts of a body nothing has been recorded about yet: each group
    /// holds its default, which states nothing a fresh body contradicts (no
    /// parameter taken over, no local borrowed), at the body's generation.
    pub fn new(body: &Body) -> Self {
        BodyFacts {
            ownership: OwnershipFacts::default(),
            ownership_at: Some(body.generation()),
        }
    }

    /// The ownership facts, without asking whether they hold at the body's
    /// generation: for a pass the runner already checked them for.
    pub fn ownership(&self) -> &OwnershipFacts {
        &self.ownership
    }

    /// The ownership facts, for a pass that records them.
    pub fn ownership_mut(&mut self) -> &mut OwnershipFacts {
        &mut self.ownership
    }

    /// The ownership facts, refused unless they hold at `body`'s generation.
    pub fn current_ownership(&self, body: &Body) -> Result<&OwnershipFacts, StaleFact> {
        self.check(Fact::Ownership, body)?;
        Ok(&self.ownership)
    }

    /// Fails when `fact` does not hold at `body`'s generation.
    pub fn check(&self, fact: Fact, body: &Body) -> Result<(), StaleFact> {
        let recorded_at = *self.stamp(fact);
        if recorded_at == Some(body.generation()) {
            return Ok(());
        }
        Err(StaleFact {
            fact,
            recorded_at,
            read_at: body.generation(),
        })
    }

    /// Marks `fact` as holding at `body`'s generation: recorded there, or
    /// carried there across a rewrite that preserves it.
    pub fn stamp_current(&mut self, fact: Fact, body: &Body) {
        *self.stamp_mut(fact) = Some(body.generation());
    }

    /// Marks `fact` as no longer holding.
    pub fn invalidate(&mut self, fact: Fact) {
        *self.stamp_mut(fact) = None;
    }

    fn stamp(&self, fact: Fact) -> &Option<u64> {
        match fact {
            Fact::Ownership => &self.ownership_at,
        }
    }

    fn stamp_mut(&mut self, fact: Fact) -> &mut Option<u64> {
        match fact {
            Fact::Ownership => &mut self.ownership_at,
        }
    }
}

/// The facts of every body of a program, in the order of the bodies they
/// describe.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgramFacts {
    bodies: Vec<BodyFacts>,
}

impl ProgramFacts {
    /// Fresh facts for each of `bodies`, in their order.
    pub fn of<K>(bodies: &[(K, Body)]) -> Self {
        ProgramFacts {
            bodies: bodies
                .iter()
                .map(|(_, body)| BodyFacts::new(body))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, BodyFacts> {
        self.bodies.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, BodyFacts> {
        self.bodies.iter_mut()
    }
}
