// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! CFG analysis utilities for MIR.
//!
//! This module provides analysis passes that compute properties of the control flow graph:
//!
//! - **Dominators**: Compute dominator trees and dominance frontiers for SSA construction
//!   and loop detection. A block `A` dominates block `B` if every path from the entry
//!   to `B` must pass through `A`.
//! - **Header reach**: Which calls a loop may make without changing the header
//!   of a collection the body holds: the collection's aliases, and whether it
//!   holds a reference of its own.
//! - **Loops**: The natural loops of a body, each found along the edges back to
//!   its header.
//! - **Traversal**: The reverse postorder a forward dataflow visits blocks in.
//!
//! These analyses are foundational for optimization passes and SSA transformation.

pub mod dominators;
pub mod header_reach;
pub mod loops;
pub mod traversal;
