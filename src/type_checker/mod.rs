// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::error::type_error::TypeError;
use std::collections::HashMap;

pub mod context;
pub mod expressions;
pub mod statements;
pub mod utils;

use context::{Context, StructDefinition, SymbolInfo, TypeDefinition, TypeRelation};

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
        let mut global_type_definitions = HashMap::new();

        // Define built-in String type
        global_type_definitions.insert(
            "String".to_string(),
            TypeDefinition::Struct(StructDefinition {
                fields: vec![("length".to_string(), Type::Int, MemberVisibility::Public)],
                generics: None,
                module: "std".to_string(),
            }),
        );

        Self {
            types: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            hierarchy: HashMap::new(),
            current_module: "Main".to_string(),
            global_scope: HashMap::new(),
            global_type_definitions,
        }
    }

    pub fn set_current_module(&mut self, name: String) {
        self.current_module = name;
    }

    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.types.get(&id)
    }

    pub fn get_variable_type(&self, name: &str) -> Option<&Type> {
        self.global_scope.get(name).map(|info| &info.ty)
    }

    /// Main entry point for type checking a program.
    pub fn check(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut context = Context::new();
        for statement in &program.body {
            // Flatten top-level blocks to ensure variables are declared in the global scope
            // This handles cases where the entire program is indented (e.g. in tests)
            if let Statement::Block(stmts) = statement {
                for stmt in stmts {
                    self.check_statement(stmt, &mut context);
                }
            } else {
                self.check_statement(statement, &mut context);
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
}
