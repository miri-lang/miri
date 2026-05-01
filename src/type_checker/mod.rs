// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type checker module for Miri.
//!
//! This module is responsible for validating the type safety of Miri programs.
//! It performs type inference, checks type compatibility, validates operations,
//! and ensures that all assignments and function calls use compatible types.
//!
//! # Module Structure
//!
//! ## Core Modules
//! - [`context`] - Type checking context (scopes, symbols, type definitions)
//! - [`expressions`] - Expression type inference
//! - [`statements`] - Statement type checking
//!
//! ## Support Modules
//! - [`builtins`] - Built-in types and functions (String, Dim3, Future, print)
//! - [`compatibility`] - Type compatibility and subtyping checks
//! - [`generics`] - Generic type inference and substitution
//! - [`operators`] - Binary and unary operator type validation
//! - [`utils`] - Type predicates, visibility, and error reporting

use crate::ast::factory::make_type;
use crate::ast::types::Type;
use crate::ast::*;
use crate::error::diagnostic::Diagnostic;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

mod builtins;
mod compatibility;
pub mod context;
pub mod expressions;
mod generics;
mod operators;
pub mod statements;
pub mod use_after_move;
pub mod utils;

use context::{Context, SymbolInfo, TypeDefinition, TypeRelation};

/// Extracts a `Type` from an AST type expression without a full `TypeChecker`.
///
/// This is used by the pipeline to translate runtime function parameter/return
/// types (which are always simple, non-generic types like `Int`, `Bool`,
/// `RawPtr`) into `Type` objects suitable for codegen translation.
///
/// Returns `None` if the expression is not a type expression.
pub fn resolve_type_name(expr: &Expression) -> Option<Type> {
    match &expr.node {
        ExpressionKind::Type(ty, _) => Some(*ty.clone()),
        _ => None,
    }
}

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
    pub warnings: Vec<Diagnostic>,
    /// Stores type hierarchy relationships (extends, implements, includes)
    pub(crate) hierarchy: HashMap<String, TypeRelation>,
    /// Name of the current module/class being checked
    pub(crate) current_module: String,
    pub(crate) global_scope: HashMap<String, SymbolInfo>,
    pub(crate) global_type_definitions: HashMap<String, TypeDefinition>,
    /// Set of modules that have been fully loaded.
    pub(crate) loaded_modules: std::collections::HashSet<String>,
    /// Stack of modules currently being loaded (used to detect circular imports).
    pub(crate) loading_stack: Vec<String>,
    /// Tracks (message, span) pairs to deduplicate errors reported multiple times
    /// for the same source location (e.g. when a type expression is resolved twice).
    pub(crate) reported_errors: std::collections::HashSet<(String, Span)>,
    /// AST statements collected from imported modules.
    /// These need to be included in MIR lowering and codegen.
    pub imported_statements: Vec<Statement>,
    /// Maps call expression IDs to their inferred generic type arguments (in declaration order).
    /// Populated when a generic function is called so MIR lowering can mangle the call target.
    pub call_generic_mappings: HashMap<usize, Vec<(String, Type)>>,
    /// Directory of the source file being compiled, used to resolve `local.*` imports.
    pub(crate) source_dir: Option<PathBuf>,
    /// Maps module alias names to their full module paths.
    /// e.g., `"M"` → `"system.math"` for `use system.math as M`.
    pub(crate) module_aliases: HashMap<String, String>,
    /// When set, errors are tagged with this (file_path, source_text) so that
    /// the formatter can display the correct source context for imported files.
    pub(crate) current_source_override: Option<(String, String)>,
    /// Tracks which type names are visible to user code. Types in
    /// `global_type_definitions` but NOT in this set are internal-only
    /// (e.g. transitive trait dependencies kept for vtable generation).
    pub(crate) visible_type_names: std::collections::HashSet<String>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    /// Creates a new type checker with built-in types and functions pre-loaded.
    ///
    /// The type checker is initialized with:
    /// - Built-in types: `String`, `Dim3`, `GpuContext`, `Kernel`, `Future<T>`
    /// - Built-in functions: `print<T>`
    pub fn new() -> Self {
        let (global_scope, global_type_definitions) = builtins::initialize_builtins();
        let visible_type_names = global_type_definitions.keys().cloned().collect();
        Self {
            types: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            hierarchy: HashMap::new(),
            current_module: "Main".to_string(),
            global_scope,
            global_type_definitions,
            loaded_modules: std::collections::HashSet::new(),
            loading_stack: Vec::new(),
            reported_errors: std::collections::HashSet::new(),
            imported_statements: Vec::new(),
            call_generic_mappings: HashMap::new(),
            source_dir: None,
            module_aliases: HashMap::new(),
            current_source_override: None,
            visible_type_names,
        }
    }

    /// Creates a new type checker with a known source-file directory.
    ///
    /// The directory is used to resolve `local.*` module imports relative to
    /// the project root (i.e. the directory that contains the entry-point file).
    pub fn with_source_dir(source_dir: PathBuf) -> Self {
        let mut tc = Self::new();
        tc.source_dir = Some(source_dir);
        tc
    }

    /// Sets the current module name for scoping declarations.
    pub fn set_current_module(&mut self, name: String) {
        self.current_module = name;
    }

    /// Returns the inferred type for a given expression ID.
    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.types.get(&id)
    }

    /// Returns the type of a global variable by name.
    pub fn get_variable_type(&self, name: &str) -> Option<&Type> {
        self.global_scope.get(name).map(|info| &info.ty)
    }

    /// Returns whether a global variable is a constant.
    pub fn is_constant(&self, name: &str) -> bool {
        self.global_scope
            .get(name)
            .map(|info| info.is_constant)
            .unwrap_or(false)
    }

    /// Returns the global type definitions.
    pub fn type_definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.global_type_definitions
    }

    /// Main entry point for type checking a program.
    pub fn check(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut context = Context::new();

        // Load the implicit prelude so that core types (e.g. String) are available
        // in every program without an explicit `use` statement.
        self.load_prelude(&mut context);

        // Pass 1: Collect all top-level declarations to support forward references.
        // This registers function signatures and type definitions (structs, enums, classes, traits).
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.collect_declaration(stmt, &mut context);
                }
            } else {
                self.collect_declaration(statement, &mut context);
            }
        }

        // Pass 2: Check all statement bodies now that all symbols are registered.
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

        // Pass 3: use-after-move analysis — runs only when type checking is clean
        // so that we don't emit spurious "consumed" errors on top of type errors.
        if self.errors.is_empty() {
            let (uam_errors, uam_warnings) = use_after_move::UseAfterMoveChecker::new(
                &self.types,
                &self.global_type_definitions,
            )
            .check_program(program);
            self.errors.extend(uam_errors);
            self.warnings.extend(uam_warnings);
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    /// Preliminary pass to register declarations without checking their bodies.
    fn collect_declaration(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::FunctionDeclaration(decl) => {
                let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: decl.generics.clone(),
                    params: decl.params.to_vec(),
                    return_type: decl.return_type.clone(),
                })));

                if context.scopes.len() == 1 {
                    self.global_scope.insert(
                        decl.name.clone(),
                        SymbolInfo::new(
                            func_type.clone(),
                            false,
                            false,
                            decl.properties.visibility.clone(),
                            self.current_module.clone(),
                            None,
                        ),
                    );
                }

                context.define(
                    decl.name.clone(),
                    SymbolInfo::new(
                        func_type,
                        false,
                        false,
                        decl.properties.visibility.clone(),
                        self.current_module.clone(),
                        None,
                    ),
                );
            }
            StatementKind::Class(class_data) => {
                // Register class name in global_type_definitions to allow references
                // during function signature resolution.
                if let Ok(name) = self.extract_type_name(&class_data.name) {
                    if !self.global_type_definitions.contains_key(name) {
                        // Extract generics before insertion to avoid borrow checker error
                        let generics = class_data
                            .generics
                            .as_ref()
                            .map(|gens| self.extract_generic_definitions(gens, context));

                        // Initial registration as an empty class to resolve basic type identity.
                        // The full class check will happen later in check_statement.
                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Class(context::ClassDefinition {
                                name: name.to_string(),
                                generics,
                                base_class: None,
                                traits: vec![],
                                fields: Vec::new(),
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                                is_abstract: class_data.is_abstract,
                                has_drop: false,
                            }),
                        );
                    }
                }
            }
            StatementKind::Struct(name_expr, generics_expr, _, _, _) => {
                if let Ok(name) = self.extract_type_name(name_expr) {
                    if !self.global_type_definitions.contains_key(name) {
                        let generics = generics_expr
                            .as_ref()
                            .map(|gens| self.extract_generic_definitions(gens, context));

                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Struct(context::StructDefinition {
                                fields: vec![],
                                generics,
                                has_drop: false,
                                module: self.current_module.clone(),
                            }),
                        );
                    }
                }
            }
            StatementKind::Enum(name_expr, generics_expr, _, _) => {
                if let Ok(name) = self.extract_type_name(name_expr) {
                    if !self.global_type_definitions.contains_key(name) {
                        let generics = generics_expr
                            .as_ref()
                            .map(|gens| self.extract_generic_definitions(gens, context));

                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Enum(context::EnumDefinition {
                                variants: BTreeMap::new(),
                                generics,
                                module: self.current_module.clone(),
                            }),
                        );
                    }
                }
            }
            StatementKind::Trait(name_expr, generics_expr, _, _, _) => {
                if let Ok(name) = self.extract_type_name(name_expr) {
                    if !self.global_type_definitions.contains_key(name) {
                        let generics = generics_expr
                            .as_ref()
                            .map(|gens| self.extract_generic_definitions(gens, context));

                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Trait(context::TraitDefinition {
                                name: name.to_string(),
                                generics,
                                parent_traits: vec![],
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                            }),
                        );
                    }
                }
            }
            StatementKind::Use(path_expr, alias) => {
                // Load imported modules early so their types are available
                self.check_use(path_expr, alias, context);
            }
            _ => {}
        }
    }
}
