// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! SSA (Static Single Assignment) form support.
//!
//! SSA is an intermediate representation property where each variable is assigned exactly
//! once. This simplifies many optimizations (constant propagation, dead code elimination)
//! by making def-use chains explicit.
//!
//! This module provides:
//!
//! - **`construction`**: Convert MIR to SSA form by:
//!   - Computing dominance frontiers
//!   - Inserting phi nodes at join points
//!   - Renaming variables to create unique definitions
//!
//! - **`destruction`**: Convert SSA back to standard MIR by:
//!   - Eliminating phi nodes
//!   - Inserting copy operations on CFG edges
//!
//! # Example SSA Form
//! ```text
//! // Before SSA          // After SSA
//! x = 1                  x_1 = 1
//! if cond:               if cond:
//!   x = 2                  x_2 = 2
//! use(x)                 x_3 = phi(x_1, x_2)
//!                        use(x_3)
//! ```

pub mod construction;
pub mod destruction;
