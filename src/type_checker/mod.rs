// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::type_error::TypeError;
use std::collections::HashMap;

pub mod context;
pub mod expressions;
pub mod statements;
pub mod utils;

use context::{Context, SymbolInfo, TypeDefinition, TypeRelation};

/// The TypeChecker struct is responsible for validating the type safety of the program.
/// It traverses the AST, infers types for expressions, and ensures that operations
/// and assignments are performed on compatible types.
#[derive(Debug)]
pub struct TypeChecker {
    /// Maps expression IDs to their inferred types.
    pub(crate) types: HashMap<usize, Type>,
    /// Collects all type errors encountered during checking.
    pub(crate) errors: Vec<TypeError>,
    /// Collects all type warnings encountered during checking.
    pub warnings: Vec<TypeError>,
    /// Stores type hierarchy relationships (extends, implements, includes)
    pub(crate) hierarchy: HashMap<String, TypeRelation>,
    /// Name of the current module/class being checked
    pub(crate) current_module: String,
    pub(crate) global_scope: HashMap<String, SymbolInfo>,
    pub(crate) global_type_definitions: HashMap<String, TypeDefinition>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            hierarchy: HashMap::new(),
            current_module: "Main".to_string(),
            global_scope: HashMap::new(),
            global_type_definitions: HashMap::new(),
        }
    }

    pub fn set_current_module(&mut self, name: String) {
        self.current_module = name;
    }

    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.types.get(&id)
    }

    /// Main entry point for type checking a program.
    pub fn check(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut context = Context::new();
        for statement in &program.body {
            self.check_statement(statement, &mut context);
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
}
