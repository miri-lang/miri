// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The symbols a compilation has given bodies, and the refusal of a second
//! definition that would be linked under a name one of them already holds.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;

use super::Symbol;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;

/// What renaming a colliding definition asks of the program.
const COLLISION_HELP: &str =
    "rename one of the two definitions so that their compiled names differ";

/// The answer to claiming a symbol for a body about to be lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// Nothing holds the symbol's link name yet; the body is lowered now.
    New,
    /// This very symbol already has its body.
    AlreadyLowered,
}

/// Two distinct symbols that spell one link name. Linking both is impossible,
/// and keeping one would run its body wherever the other is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolCollision {
    /// The symbol claimed first.
    pub existing: Symbol,
    /// The symbol whose claim was refused.
    pub incoming: Symbol,
    /// The link name both spell.
    pub link_name: String,
}

impl SymbolCollision {
    /// The refusal of the program, at `span`, the definition of `incoming`.
    pub fn refusal(&self, span: Span) -> LoweringError {
        LoweringError::coded(
            DiagnosticCode::MirSymbolCollision,
            self.to_string(),
            span,
            Some(COLLISION_HELP.to_string()),
        )
    }
}

impl fmt::Display for SymbolCollision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} and {} compile to the same symbol `{}`",
            self.existing.written(),
            self.incoming.written(),
            self.link_name
        )
    }
}

/// Every symbol a compilation has claimed, keyed by the link name it spells.
#[derive(Debug, Default)]
pub struct SymbolTable {
    by_link_name: HashMap<String, Symbol>,
}

impl SymbolTable {
    /// Claim `symbol` for a body: [`Claim::New`] the first time,
    /// [`Claim::AlreadyLowered`] after that, and a [`SymbolCollision`] when a
    /// different symbol already holds its link name.
    pub fn claim(&mut self, symbol: &Symbol) -> Result<Claim, Box<SymbolCollision>> {
        match self.by_link_name.entry(symbol.link_name()) {
            Entry::Vacant(slot) => {
                slot.insert(symbol.clone());
                Ok(Claim::New)
            }
            Entry::Occupied(slot) if slot.get() == symbol => Ok(Claim::AlreadyLowered),
            Entry::Occupied(slot) => Err(Box::new(SymbolCollision {
                existing: slot.get().clone(),
                incoming: symbol.clone(),
                link_name: slot.key().clone(),
            })),
        }
    }

    /// Whether `symbol` itself, not merely its link name, has been claimed.
    pub fn is_claimed(&self, symbol: &Symbol) -> bool {
        self.by_link_name
            .get(&symbol.link_name())
            .is_some_and(|claimed| claimed == symbol)
    }
}
