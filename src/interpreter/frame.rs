// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Call frame for function execution.

use crate::interpreter::value::Value;
use crate::mir::{BasicBlock, Body};

/// A call frame representing a function invocation.
#[derive(Debug)]
pub struct Frame {
    /// Local variable storage, indexed by Local index.
    /// Local 0 is the return value, 1..=arg_count are parameters.
    pub locals: Vec<Option<Value>>,
    /// Current basic block index.
    pub current_block: BasicBlock,
    /// Current statement index within the block.
    pub stmt_index: usize,
    /// Last assigned value (for REPL expression evaluation).
    pub last_assigned: Option<Value>,
}

impl Frame {
    /// Create a new frame for the given function body with arguments.
    pub fn new(body: &Body, args: Vec<Value>) -> Self {
        // Initialize locals: all None initially
        let mut locals = vec![None; body.local_decls.len()];

        // Set up arguments (locals 1..=arg_count)
        for (i, arg) in args.into_iter().enumerate() {
            let local_idx = i + 1; // Skip return place (local 0)
            if local_idx < locals.len() {
                locals[local_idx] = Some(arg);
            }
        }

        Self {
            locals,
            current_block: BasicBlock(0), // Start at entry block
            stmt_index: 0,
            last_assigned: None,
        }
    }

    /// Get the value of a local variable.
    pub fn get_local(&self, idx: usize) -> Option<&Value> {
        self.locals.get(idx).and_then(|v| v.as_ref())
    }

    /// Set the value of a local variable.
    pub fn set_local(&mut self, idx: usize, value: Value) {
        if idx < self.locals.len() {
            // Track last assigned for REPL expression evaluation
            self.last_assigned = Some(value.clone());
            self.locals[idx] = Some(value);
        }
    }

    /// Get the return value (local 0) or last assigned value for script mode.
    pub fn get_return_value(&self) -> Value {
        // First try _0 (explicit return)
        if let Some(Some(v)) = self.locals.first() {
            return v.clone();
        }
        // Fall back to last assigned (for scripts/REPL)
        self.last_assigned.clone().unwrap_or(Value::None)
    }

    /// Jump to a new basic block.
    pub fn goto(&mut self, block: BasicBlock) {
        self.current_block = block;
        self.stmt_index = 0;
    }

    /// Advance to the next statement.
    pub fn next_stmt(&mut self) {
        self.stmt_index += 1;
    }
}
