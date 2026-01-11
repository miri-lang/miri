// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! MIR Interpreter for Miri.
//!
//! This module provides an interpreter that executes MIR (Mid-level IR) directly,
//! enabling fast REPL evaluation and development runs without compilation overhead.
//!
//! The interpreter shares the same frontend (lexer, parser, type checker) and MIR
//! lowering as the compiled backends, ensuring consistent semantics.

mod eval;
mod frame;
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
}

impl Interpreter {
    /// Create a new interpreter instance.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
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
            .ok_or_else(|| InterpreterError::UndefinedFunction(name.to_string()))?
            .clone();

        eval::execute_function(self, &body, args)
    }

    /// Execute the "main" function if it exists.
    pub fn run_main(&mut self) -> Result<Value, InterpreterError> {
        self.call("main", vec![])
    }
}
