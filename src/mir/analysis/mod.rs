// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! CFG analysis utilities for MIR.
//!
//! This module provides analysis passes that compute properties of the control flow graph:
//!
//! - **Dominators**: Compute dominator trees and dominance frontiers for SSA construction
//!   and loop detection. A block `A` dominates block `B` if every path from the entry
//!   to `B` must pass through `A`.
//!
//! These analyses are foundational for optimization passes and SSA transformation.

pub mod dominators;
