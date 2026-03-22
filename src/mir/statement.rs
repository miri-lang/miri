// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::place::Place;
use crate::mir::rvalue::Rvalue;
use std::fmt;

/// A statement in a basic block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            StatementKind::Assign(place, rvalue) => write!(f, "{} = {}", place, rvalue),
            StatementKind::Reassign(place, rvalue) => {
                write!(f, "reassign {} = {}", place, rvalue)
            }
            StatementKind::StorageLive(place) => write!(f, "StorageLive({})", place),
            StatementKind::StorageDead(place) => write!(f, "StorageDead({})", place),
            StatementKind::IncRef(place) => write!(f, "IncRef({})", place),
            StatementKind::DecRef(place) => write!(f, "DecRef({})", place),
            StatementKind::Dealloc(place) => write!(f, "Dealloc({})", place),
            StatementKind::Nop => write!(f, "nop"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatementKind {
    /// Assign an Rvalue to a Place (initial assignment; no DecRef of old value).
    Assign(Place, Rvalue),
    /// Reassign an Rvalue to an already-initialised managed Place.
    ///
    /// Semantically identical to `Assign` in codegen, but signals to the Perceus
    /// RC pass that the destination already holds a live reference that must be
    /// DecRef'd before the new value is written.  Lowering emits this variant
    /// whenever it knows the target local is being *overwritten* (i.e. it was
    /// already assigned at least once in its current lifetime).
    Reassign(Place, Rvalue),
    /// A storage live statement (start of variable scope).
    StorageLive(Place),
    /// A storage dead statement (end of variable scope).
    StorageDead(Place),
    /// Unconditional increment of reference count.
    IncRef(Place),
    /// Unconditional decrement of reference count. May trigger deallocation.
    DecRef(Place),
    /// Explicit deallocation (used when optimization proves uniqueness).
    Dealloc(Place),
    /// No-op.
    Nop,
}
