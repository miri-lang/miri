// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collections a body reads the header of once per loop.
//!
//! Each is a [`StatementKind::ReadHeader`] at the end of its loop's
//! preheader, so a pass that moves or copies blocks carries it the way it
//! carries a jump. This module reads those statements back as records.

use std::fmt;

use crate::mir::{BasicBlock, Body, Local, StatementKind};

/// An array or a list a loop indexes without changing its header: the data
/// pointer, the length and the slot size read on entry hold on every
/// iteration. A view of a `ReadHeader` statement, with the block holding it
/// as the preheader, read off the body by [`stable_collections`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableCollection {
    /// The local holding the collection.
    pub local: Local,
    /// The block every entry into the loop passes through, ending in a jump
    /// to `header`, where the header is read.
    pub preheader: BasicBlock,
    /// The loop's header.
    pub header: BasicBlock,
    /// Every block of the loop, the header included, in ascending order.
    pub blocks: Vec<BasicBlock>,
}

impl fmt::Display for StableCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let blocks: Vec<String> = self.blocks.iter().map(ToString::to_string).collect();
        write!(
            f,
            "stable {} from {} over {}: [{}]",
            self.local,
            self.preheader,
            self.header,
            blocks.join(", ")
        )
    }
}

/// Every collection a loop of `body` leaves the header of unchanged,
/// one per `ReadHeader` statement, in block order. Decided by
/// `crate::mir::optimization::stable_collections`, re-derived by
/// `crate::mir::verify::verify_stable_collections`.
pub fn stable_collections(body: &Body) -> Vec<StableCollection> {
    let mut found = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for statement in &data.statements {
            if let StatementKind::ReadHeader(read) = &statement.kind {
                found.push(StableCollection {
                    local: read.collection.local,
                    preheader: BasicBlock(block),
                    header: read.header,
                    blocks: read.blocks.clone(),
                });
            }
        }
    }
    found
}
