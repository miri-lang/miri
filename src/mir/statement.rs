// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::mir::block::BasicBlock;
use crate::mir::place::Place;
use crate::mir::rvalue::Rvalue;
use std::fmt;

/// A statement in a basic block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

impl Statement {
    /// The place this statement stores a value into: an assignment's
    /// destination; `None` for any other statement.
    pub fn written_place(&self) -> Option<&Place> {
        match &self.kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => Some(place),
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_, _)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::ReadHeader(_)
            | StatementKind::Nop => None,
        }
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            StatementKind::Assign(place, rvalue) => write!(f, "{} = {}", place, rvalue),
            StatementKind::Reassign(place, rvalue) => {
                write!(f, "reassign {} = {}", place, rvalue)
            }
            StatementKind::StorageLive(place) => write!(f, "StorageLive({})", place),
            StatementKind::StorageDead(place, EndCause::Scope) => {
                write!(f, "StorageDead({})", place)
            }
            StatementKind::StorageDead(place, EndCause::Consumed) => {
                write!(f, "StorageDead({}, consumed)", place)
            }
            StatementKind::StorageDead(place, EndCause::Moved) => {
                write!(f, "StorageDead({}, moved)", place)
            }
            StatementKind::IncRef(place) => write!(f, "IncRef({})", place),
            StatementKind::DecRef(place) => write!(f, "DecRef({})", place),
            StatementKind::Dealloc(place) => write!(f, "Dealloc({})", place),
            StatementKind::ReadHeader(read) => write!(f, "{read}"),
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
    /// Semantically identical to `Assign` in codegen, but signals to the RC insertion
    /// RC pass that the destination already holds a live reference that must be
    /// DecRef'd before the new value is written.  Lowering emits this variant
    /// whenever it knows the target local is being *overwritten* (i.e. it was
    /// already assigned at least once in its current lifetime).
    Reassign(Place, Rvalue),
    /// A storage live statement (start of variable scope).
    StorageLive(Place),
    /// The end of a binding's storage, and with it the place its release
    /// goes: where its scope ends, or where a consuming site gave its value
    /// up ([`EndCause`]).
    StorageDead(Place, EndCause),
    /// Unconditional increment of reference count.
    IncRef(Place),
    /// Unconditional decrement of reference count. May trigger deallocation.
    DecRef(Place),
    /// Explicit deallocation (used when optimization proves uniqueness).
    Dealloc(Place),
    /// Reads the header of the array or list `read.collection` — its data
    /// pointer, length and slot size — once, at the end of the preheader of
    /// a loop that leaves the header unchanged, for every access in
    /// `read.blocks` to use. Changes nothing.
    ReadHeader(HeaderRead),
    /// No-op.
    Nop,
}

impl StatementKind {
    /// The end of `place`'s storage where its scope ends.
    pub fn storage_dead(place: Place) -> StatementKind {
        StatementKind::StorageDead(place, EndCause::Scope)
    }
}

/// Why a binding's storage ends.
///
/// A scope end and a consuming end release the binding there; a moved end
/// releases nothing, its reference having left through a `move`. A consuming
/// or moved end is written where a site gives the binding's value up — a call
/// that keeps it, a `let` moving it into a new binding, `value.drop()` — and
/// drop elaboration decides, from those ends alone, which later scope ends
/// still release it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndCause {
    /// The scope declaring the binding ends.
    Scope,
    /// A consuming site gave the binding's value up, keeping a reference of
    /// its own, so the binding's reference is released here.
    Consumed,
    /// A consuming site took the binding's own reference with a `move`, so
    /// the binding ends here and releases nothing.
    Moved,
}

impl EndCause {
    /// Whether a site gave the binding's value up here, rather than its
    /// scope ending.
    pub fn is_consuming(self) -> bool {
        match self {
            EndCause::Consumed | EndCause::Moved => true,
            EndCause::Scope => false,
        }
    }

    /// Whether ending the binding here releases the reference it holds.
    pub fn releases(self) -> bool {
        match self {
            EndCause::Scope | EndCause::Consumed => true,
            EndCause::Moved => false,
        }
    }
}

/// What a [`StatementKind::ReadHeader`] reads and for which blocks: the
/// collection, the loop's header and every block of the loop, the header
/// included, in ascending order. The block holding the statement is the
/// loop's preheader. At most one read covers a block for one collection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderRead {
    pub collection: Place,
    pub header: BasicBlock,
    pub blocks: Vec<BasicBlock>,
}

impl fmt::Display for HeaderRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let blocks: Vec<String> = self.blocks.iter().map(ToString::to_string).collect();
        write!(
            f,
            "ReadHeader({}) over {}: [{}]",
            self.collection,
            self.header,
            blocks.join(", ")
        )
    }
}
