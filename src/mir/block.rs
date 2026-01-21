// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Basic block types for MIR control flow graphs.
//!
//! A basic block is a sequence of statements with a single entry point
//! and a single exit point (the terminator).

use crate::mir::statement::Statement;
use crate::mir::terminator::Terminator;
use std::fmt;

/// A basic block identifier.
///
/// Basic blocks are indexed starting from 0. Block 0 is always the entry
/// block of a function body. The index is used to reference blocks within
/// terminators (e.g., `Goto { target: BasicBlock(1) }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlock(pub usize);

impl fmt::Display for BasicBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

/// The data for a single basic block in the control flow graph.
///
/// A basic block contains:
/// - A sequence of [`Statement`]s executed in order
/// - An optional [`Terminator`] that transfers control to another block
/// - A flag indicating if this block is for cleanup (e.g., unwinding)
///
/// All basic blocks except possibly the last one being constructed should
/// have a terminator. The [`Body::validate`](crate::mir::Body::validate)
/// method checks this invariant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasicBlockData {
    /// Statements executed sequentially within this block.
    pub statements: Vec<Statement>,
    /// The terminator that ends this block and transfers control.
    /// Should be `Some` for all completed blocks.
    pub terminator: Option<Terminator>,
    /// Whether this block is for cleanup (e.g., stack unwinding).
    pub is_cleanup: bool,
}

impl BasicBlockData {
    pub fn new(terminator: Option<Terminator>) -> Self {
        Self {
            // Most basic blocks have a few statements
            statements: Vec::with_capacity(8),
            terminator,
            is_cleanup: false,
        }
    }
}
