// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::types::{Type, TypeDeclarationKind};
use crate::ast::{types::TypeKind, *};
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
        let (global_scope, global_type_definitions) = Self::initialize_builtins();

        Self {
            types: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            hierarchy: HashMap::new(),
            current_module: "Main".to_string(),
            global_scope,
            global_type_definitions,
        }
    }

    fn initialize_builtins() -> (HashMap<String, SymbolInfo>, HashMap<String, TypeDefinition>) {
        let mut global_type_definitions = HashMap::new();

        // Define built-in String type
        global_type_definitions.insert(
            "String".to_string(),
            TypeDefinition::Struct(StructDefinition {
                fields: vec![(
                    "length".to_string(),
                    crate::ast::factory::make_type(TypeKind::Int),
                    MemberVisibility::Public,
                )],
                generics: None,
                module: "std".to_string(),
            }),
        );

        let mut global_scope = HashMap::new();

        // Define built-in print function: fn print<T>(value T)
        let generic_t = crate::ast::factory::make_type(TypeKind::Generic(
            "T".to_string(),
            None,
            TypeDeclarationKind::None,
        ));
        let generic_decl = crate::ast::factory::generic_type_expression(
            crate::ast::factory::identifier("T"),
            None,
            TypeDeclarationKind::None,
        );

        global_scope.insert(
            "print".to_string(),
            SymbolInfo {
                ty: crate::ast::factory::make_type(TypeKind::Function(
                    Some(vec![generic_decl]),
                    vec![Parameter {
                        name: "value".to_string(),
                        typ: Box::new(crate::ast::factory::type_expr_non_null(generic_t)),
                        guard: None,
                        default_value: None,
                    }],
                    Some(Box::new(crate::ast::factory::type_expr_non_null(
                        crate::ast::factory::make_type(TypeKind::Void),
                    ))),
                )),
                mutable: false,
                visibility: MemberVisibility::Public,
                module: "std".to_string(),
            },
        );

        (global_scope, global_type_definitions)
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
            if let StatementKind::Block(stmts) = &statement.node {
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
