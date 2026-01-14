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
            StatementKind::StorageLive(place) => write!(f, "StorageLive({})", place),
            StatementKind::StorageDead(place) => write!(f, "StorageDead({})", place),
            StatementKind::Nop => write!(f, "nop"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatementKind {
    /// Assign an Rvalue to a Place.
    Assign(Place, Rvalue),
    /// A storage live statement (start of variable scope).
    StorageLive(Place),
    /// A storage dead statement (end of variable scope).
    StorageDead(Place),
    /// No-op.
    Nop,
}
