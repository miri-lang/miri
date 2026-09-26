// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The symbols a compilation has given bodies, and the refusal of a second
//! definition that would be declared under a name one of them already holds.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;

use super::Symbol;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;

/// What renaming a colliding definition asks of the program.
const COLLISION_HELP: &str =
    "rename one of the two definitions so that their compiled names differ";

/// What a definition at a type argument with no name asks of the program.
const UNNAMEABLE_HELP: &str =
    "instantiate at a type the compiler can name; wrap the value in a class or struct and instantiate at that";

/// The names a [`SymbolTable`] keeps apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Namespace {
    /// The names the linker knows compiled bodies and data by.
    #[default]
    Link,
    /// The names a WGSL module declares kernel entry points and helpers under.
    Wgsl,
}

impl Namespace {
    fn spell(self, symbol: &Symbol) -> String {
        match self {
            Namespace::Link => symbol.link_name(),
            Namespace::Wgsl => symbol.wgsl_name(),
        }
    }
}

/// The answer to claiming a symbol for a body about to be lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// Nothing holds the symbol's name yet; the body is lowered now.
    New,
    /// This very symbol already has its body.
    AlreadyLowered,
}

/// Why a symbol could not be claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimRefusal {
    /// A different symbol already holds the name this one spells.
    Collision(SymbolCollision),
    /// The symbol has a type argument with no name, so it stands for every
    /// instantiation at such a type and cannot hold the body of any one.
    Unnameable(Symbol),
}

impl ClaimRefusal {
    /// The refusal of the program, at `span`, the definition claimed.
    pub fn refusal(&self, span: Span) -> LoweringError {
        match self {
            ClaimRefusal::Collision(collision) => collision.refusal(span),
            ClaimRefusal::Unnameable(symbol) => LoweringError::coded(
                DiagnosticCode::MirInvalidInstantiationArgument,
                format!(
                    "{} has a type argument with no name the compiler can compile a body at",
                    symbol.written()
                ),
                span,
                Some(UNNAMEABLE_HELP.to_string()),
            ),
        }
    }
}

/// Two distinct symbols that spell one name in one namespace. Declaring both
/// is impossible, and keeping one would run its body wherever the other is
/// called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolCollision {
    /// The symbol claimed first.
    pub existing: Symbol,
    /// The symbol whose claim was refused.
    pub incoming: Symbol,
    /// The name both spell.
    pub name: String,
    /// Where that name is declared.
    pub namespace: Namespace,
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
        let existing = self.existing.written();
        let incoming = self.incoming.written();
        match self.namespace {
            Namespace::Link => write!(
                f,
                "{existing} and {incoming} compile to the same symbol `{}`",
                self.name
            ),
            Namespace::Wgsl => write!(
                f,
                "{existing} and {incoming} are both reached from GPU code, \
                 where both are declared as `{}`",
                self.name
            ),
        }
    }
}

/// Every symbol a compilation has claimed in one [`Namespace`], keyed by the
/// name it spells there.
#[derive(Debug, Default)]
pub struct SymbolTable {
    namespace: Namespace,
    by_name: HashMap<String, Symbol>,
    claimed: HashSet<Symbol>,
    /// The declaration, by statement id, each symbol claimed through
    /// [`SymbolTable::claim_definition`] holds the body of.
    definitions: HashMap<Symbol, usize>,
}

impl SymbolTable {
    /// An empty table keeping apart the names spelled in `namespace`.
    pub fn new(namespace: Namespace) -> Self {
        Self {
            namespace,
            ..Self::default()
        }
    }

    /// Claim `symbol` for a body: [`Claim::New`] the first time,
    /// [`Claim::AlreadyLowered`] after that. A different symbol already
    /// holding its name, or a symbol with a type argument that has no name,
    /// is refused.
    pub fn claim(&mut self, symbol: &Symbol) -> Result<Claim, Box<ClaimRefusal>> {
        if symbol.has_an_unnameable_argument() {
            return Err(Box::new(ClaimRefusal::Unnameable(symbol.clone())));
        }
        if self.claimed.contains(symbol) {
            return Ok(Claim::AlreadyLowered);
        }
        match self.by_name.entry(self.namespace.spell(symbol)) {
            Entry::Vacant(slot) => {
                slot.insert(symbol.clone());
                self.claimed.insert(symbol.clone());
                Ok(Claim::New)
            }
            Entry::Occupied(slot) => Err(Box::new(ClaimRefusal::Collision(SymbolCollision {
                existing: slot.get().clone(),
                incoming: symbol.clone(),
                name: slot.key().clone(),
                namespace: self.namespace,
            }))),
        }
    }

    /// Claim `symbol` for the body of the definition at `span`: whether that
    /// body is still to be lowered. A refused claim refuses the program at
    /// `span`.
    pub fn claim_at(&mut self, symbol: &Symbol, span: Span) -> Result<bool, LoweringError> {
        match self.claim(symbol) {
            Ok(Claim::New) => Ok(true),
            Ok(Claim::AlreadyLowered) => Ok(false),
            Err(refusal) => Err(refusal.refusal(span)),
        }
    }

    /// Claim `symbol` for the body of the declaration statement `definition`
    /// at `span`: whether that body is still to be lowered. The same
    /// declaration claiming again finds its body lowered. A different
    /// declaration reaching a symbol one already holds means two definitions
    /// were taken for one; keeping either would run its body wherever the
    /// other is called, so that is refused rather than one silently dropped.
    pub fn claim_definition(
        &mut self,
        symbol: &Symbol,
        definition: usize,
        span: Span,
    ) -> Result<bool, LoweringError> {
        if self.claim_at(symbol, span)? {
            self.definitions.insert(symbol.clone(), definition);
            return Ok(true);
        }
        match self.definitions.get(symbol) {
            Some(&holder) if holder != definition => Err(LoweringError::internal(
                DiagnosticCode::MirSymbolCollision,
                format!("two declarations were both taken for {}", symbol.written()),
                span,
            )),
            Some(_) | None => Ok(false),
        }
    }

    /// Whether `symbol` itself, not merely its name, has been claimed.
    pub fn is_claimed(&self, symbol: &Symbol) -> bool {
        self.claimed.contains(symbol)
    }
}
