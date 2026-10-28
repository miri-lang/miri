// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! CFG analysis utilities for MIR.
//!
//! This module provides analysis passes that compute properties of the control flow graph:
//!
//! - **Copies read in place**: The copies of a value held as its bytes whose
//!   every read can take its bytes from where they were copied from.
//! - **Dominators**: Compute dominator trees and dominance frontiers for SSA construction
//!   and loop detection. A block `A` dominates block `B` if every path from the entry
//!   to `B` must pass through `A`.
//! - **Element matching**: What code the runtime runs when it matches or
//!   orders the elements of a container.
//! - **Heap effects**: Which calls may release a value their caller lends
//!   them, decided over the whole program.
//! - **Header reach**: Which calls a loop may make without changing the header
//!   of a collection the body holds: the collection's aliases, and whether it
//!   holds a reference of its own.
//! - **Loops**: The natural loops of a body, each found along the edges back to
//!   its header, and the comparison of two locals a block switches on.
//! - **Non-negative locals**: The integer locals no definition can make
//!   negative.
//! - **Stable headers**: Whether a loop leaves the header of a collection it
//!   indexes unchanged.
//! - **Traversal**: The reverse postorder a forward dataflow visits blocks in.
//!
//! These analyses are foundational for optimization passes and SSA transformation.

pub mod copies_read_in_place;
pub mod dominators;
pub mod element_matching;
pub mod header_reach;
pub mod heap_effects;
pub mod loops;
pub mod non_negative;
pub mod stable_headers;
pub mod traversal;
