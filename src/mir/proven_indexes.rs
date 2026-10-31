// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where a body's index accesses proven in bounds are.
//!
//! The fact lives on the access itself, as [`Bounds::Proven`] on the
//! [`PlaceElem::Index`] of the collection, so a pass that moves, copies or
//! rewrites a statement carries it with the statement. This module only
//! reads those flags out, located where they stand now, and sets one at a
//! located access.

use std::fmt;

use crate::mir::place::PlaceContext;
use crate::mir::visitor::{MutVisitor, Visitor};
use crate::mir::{BasicBlock, BasicBlockData, Body, Bounds, Local, Place, PlaceElem};

/// An index into a collection proven below its length and non-negative:
/// `collection[index]` at `position` of `block`, the terminator counted as
/// the statement after the last. A location read off the body as it stands,
/// never stored beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InBounds {
    pub block: BasicBlock,
    pub position: usize,
    pub collection: Local,
    pub index: Local,
}

impl fmt::Display for InBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "in_bounds {}[{}]: {}[{}]",
            self.block, self.position, self.collection, self.index
        )
    }
}

/// Every access of `body` an index of its collection marks proven, in block
/// and position order, each named once.
pub fn proven_indexes(body: &Body) -> Vec<InBounds> {
    let mut found = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for position in 0..=data.statements.len() {
            let mut roots = ProvenRoots::default();
            visit_position(&mut roots, BasicBlock(block), data, position);
            for (collection, index) in roots.roots {
                let site = InBounds {
                    block: BasicBlock(block),
                    position,
                    collection,
                    index,
                };
                if !found.contains(&site) {
                    found.push(site);
                }
            }
        }
    }
    found
}

/// Marks proven every access `collection[index]` at `site` makes with the
/// index at its first projection. Returns false when none is there.
pub fn mark_in_bounds(body: &mut Body, site: InBounds) -> bool {
    let Some(data) = body.basic_blocks.get_mut(site.block.0) else {
        return false;
    };
    let mut marker = Marker {
        collection: site.collection,
        index: site.index,
        marked: false,
    };
    if let Some(statement) = data.statements.get_mut(site.position) {
        marker.visit_statement(site.block, statement);
    } else if site.position == data.statements.len() {
        if let Some(terminator) = &mut data.terminator {
            marker.visit_terminator(site.block, terminator);
        }
    }
    marker.marked
}

/// Visits the statement at `position` of `data`, or its terminator when
/// `position` is the number of statements.
fn visit_position(
    visitor: &mut impl Visitor,
    block: BasicBlock,
    data: &BasicBlockData,
    position: usize,
) {
    match data.statements.get(position) {
        Some(statement) => visitor.visit_statement(block, statement),
        None => {
            if let Some(terminator) = &data.terminator {
                visitor.visit_terminator(block, terminator);
            }
        }
    }
}

/// The collection and index of every place whose first projection is an
/// index marked proven.
#[derive(Default)]
struct ProvenRoots {
    roots: Vec<(Local, Local)>,
}

impl Visitor for ProvenRoots {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if let Some(PlaceElem::Index(index, Bounds::Proven)) = place.projection.first() {
            self.roots.push((place.local, *index));
        }
    }
}

struct Marker {
    collection: Local,
    index: Local,
    marked: bool,
}

impl MutVisitor for Marker {
    fn visit_place(&mut self, place: &mut Place, _context: PlaceContext, _location: BasicBlock) {
        if place.local != self.collection {
            return;
        }
        if let Some(PlaceElem::Index(index, bounds)) = place.projection.first_mut() {
            if *index == self.index {
                *bounds = Bounds::Proven;
                self.marked = true;
            }
        }
    }
}
