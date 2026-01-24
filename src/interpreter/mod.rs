// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR Interpreter for Miri.
//!
//! This module provides an interpreter that executes MIR (Mid-level IR) directly,
//! enabling fast REPL evaluation and development runs without compilation overhead.
//!
//! The interpreter shares the same frontend (lexer, parser, type checker) and MIR
//! lowering as the compiled backends, ensuring consistent semantics.

mod eval;
#[cfg(test)]
mod tests;
mod value;

// Re-export InterpreterError from centralized error module
pub use crate::error::InterpreterError;
pub use value::Value;

use crate::mir::Body;
use std::collections::HashMap;

/// MIR Interpreter.
///
/// Executes MIR function bodies directly without compilation to machine code.
/// Useful for REPL, debugging, and fast development iteration.
#[derive(Debug, Default)]
pub struct Interpreter {
    /// Available function bodies, keyed by name.
    functions: HashMap<String, Body>,
    /// Heap for dynamic allocation.
    /// Maps allocation ID to (Value, ReferenceCount).
    heap: HashMap<usize, (Value, usize)>,
    /// Next available allocation ID.
    next_alloc_id: usize,
}

impl Interpreter {
    /// Create a new interpreter instance.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            heap: HashMap::new(),
            next_alloc_id: 1,
        }
    }

    /// Allocate a value on the heap, returning its ID.
    /// The return value starts with ref count 1.
    pub fn heap_alloc(&mut self, value: Value) -> usize {
        let id = self.next_alloc_id;
        self.next_alloc_id += 1;
        self.heap.insert(id, (value, 1));
        id
    }

    /// Get a reference to a value on the heap.
    pub fn heap_get(&self, id: usize) -> Option<&Value> {
        self.heap.get(&id).map(|(v, _)| v)
    }

    /// Get a mutable reference to a value on the heap.
    pub fn heap_get_mut(&mut self, id: usize) -> Option<&mut Value> {
        self.heap.get_mut(&id).map(|(v, _)| v)
    }

    /// Increment reference count definition.
    pub fn heap_inc_ref(&mut self, id: usize) {
        if let Some((_, rc)) = self.heap.get_mut(&id) {
            *rc += 1;
        }
    }

    /// Decrement reference count. Returns true if count reached 0 and value was dropped.
    pub fn heap_dec_ref(&mut self, id: usize) -> bool {
        if let Some((_, rc)) = self.heap.get_mut(&id) {
            *rc -= 1;
            if *rc == 0 {
                self.heap.remove(&id);
                return true;
            }
        }
        false
    }

    /// Take a value from the heap (temporarily removing it).
    pub fn heap_take(&mut self, id: usize) -> Option<(Value, usize)> {
        self.heap.remove(&id)
    }

    /// Put a value back into the heap.
    pub fn heap_put(&mut self, id: usize, value: Value, rc: usize) {
        self.heap.insert(id, (value, rc));
    }

    /// Force deallocation (unsafe if refs exist).
    pub fn heap_dealloc(&mut self, id: usize) {
        self.heap.remove(&id);
    }

    /// Load function bodies into the interpreter.
    pub fn load_functions(&mut self, bodies: Vec<(String, Body)>) {
        for (name, body) in bodies {
            self.functions.insert(name, body);
        }
    }

    /// Get a function body by name.
    pub fn get_function(&self, name: &str) -> Option<&Body> {
        self.functions.get(name)
    }

    /// Call a function by name with the given arguments.
    pub fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, InterpreterError> {
        let body = self
            .functions
            .get(name)
            .ok_or_else(|| InterpreterError::undefined_function(name.to_string()))?
            .clone();

        eval::execute_function(self, &body, args)
    }

    /// Execute the "main" function if it exists.
    pub fn run_main(&mut self) -> Result<Value, InterpreterError> {
        self.call("main", vec![])
    }
}
