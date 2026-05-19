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
pub mod escape_analysis;
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
    /// Names of classes/traits inserted by the cross-module pre-pass as
    /// partial placeholders so forward references resolve during recursive
    /// module loading. `check_class` / `check_trait` recognize members of
    /// this set as overwritable and remove the name on full registration.
    pub(crate) pre_registered_types: std::collections::HashSet<String>,
    /// Source text of the entry-point file, populated by the pipeline right
    /// before MIR lowering. Used by lowering passes (notably the testing
    /// intrinsic lowering) to convert byte spans into human-readable line
    /// numbers in runtime diagnostic messages.
    pub entry_source: Option<std::rc::Rc<str>>,
    /// Source path of the entry-point file. See [`entry_source`].
    pub entry_source_path: Option<std::rc::Rc<str>>,
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
            pre_registered_types: std::collections::HashSet::new(),
            entry_source: None,
            entry_source_path: None,
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

    /// Returns the module name where the given variable/function is defined.
    pub fn get_variable_module(&self, name: &str) -> Option<&str> {
        self.global_scope.get(name).map(|info| info.module.as_str())
    }

    /// Returns whether a global variable is a constant.
    pub fn is_constant(&self, name: &str) -> bool {
        self.global_scope
            .get(name)
            .map(|info| info.is_constant)
            .unwrap_or(false)
    }

    /// Returns whether a global variable is an intrinsic.
    pub fn is_intrinsic(&self, name: &str) -> bool {
        self.global_scope
            .get(name)
            .map(|info| info.is_intrinsic)
            .unwrap_or(false)
    }

    /// Returns the global type definitions.
    pub fn type_definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.global_type_definitions
    }

    /// Main entry point for type checking a program.
    pub fn check(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut context = Context::new();
        self.load_prelude(&mut context);

        self.run_pass_collect_type_shells(program);
        self.run_pass_collect_declarations(program, &mut context);
        self.run_pass_check_bodies(program, &mut context);
        self.run_pass_escape_summaries(program, &mut context);
        self.run_pass_use_after_move(program, &context);

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn run_pass_collect_type_shells(&mut self, program: &Program) {
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.collect_type_shells(stmt);
                }
            } else {
                self.collect_type_shells(statement);
            }
        }
    }

    fn run_pass_collect_declarations(&mut self, program: &Program, context: &mut Context) {
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.collect_declaration(stmt, context);
                }
            } else {
                self.collect_declaration(statement, context);
            }
        }
    }

    fn run_pass_check_bodies(&mut self, program: &Program, context: &mut Context) {
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.check_statement(stmt, context);
                }
            } else {
                self.check_statement(statement, context);
            }
        }
    }

    fn run_pass_escape_summaries(&mut self, program: &Program, context: &mut Context) {
        if !self.errors.is_empty() {
            return;
        }
        let ffi_summaries = std::mem::take(&mut context.escape_summaries);
        context.escape_summaries = escape_analysis::compute_escape_summaries(
            &program.body,
            &self.types,
            &self.global_type_definitions,
            ffi_summaries,
        );
    }

    fn run_pass_use_after_move(&mut self, program: &Program, context: &Context) {
        if !self.errors.is_empty() {
            return;
        }
        let (uam_errors, uam_warnings) = use_after_move::UseAfterMoveChecker::new(
            &self.types,
            &self.global_type_definitions,
            &context.escape_summaries,
        )
        .check_program(program);
        self.errors.extend(uam_errors);
        self.warnings.extend(uam_warnings);
    }

    /// Phase 1a — register class/trait/struct/enum names as empty shells.
    /// Runs before `collect_declaration` so that method signatures in later
    /// declarations can resolve forward references to types declared later
    /// in the same module (or in a module that recursively imports us back).
    pub(crate) fn collect_type_shells(&mut self, statement: &Statement) {
        let mut context = Context::new();
        let context = &mut context;
        match &statement.node {
            StatementKind::Class(class_data) => self.shell_class(class_data, context),
            StatementKind::Trait(name_expr, generics_expr, _, _, _) => {
                self.shell_trait(name_expr, generics_expr.as_ref(), context);
            }
            StatementKind::Struct(name_expr, generics_expr, _, _, _) => {
                self.shell_struct(name_expr, generics_expr.as_ref(), context);
            }
            StatementKind::Enum(name_expr, generics_expr, _, _, _, _) => {
                self.shell_enum(name_expr, generics_expr.as_ref(), context);
            }
            _ => {}
        }
    }

    fn shell_class(
        &mut self,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(&class_data.name) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let name = name.to_string();
        let generics = class_data
            .generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));
        let base_class_name: Option<String> = class_data
            .base_class
            .as_ref()
            .and_then(|b| self.extract_type_name(b).ok().map(String::from));
        let trait_names: Vec<String> = class_data
            .traits
            .iter()
            .filter_map(|t| self.extract_type_name(t).ok().map(String::from))
            .collect();
        self.register_type_definition(
            name.clone(),
            TypeDefinition::Class(context::ClassDefinition {
                name: name.clone(),
                generics,
                base_class: base_class_name,
                base_class_args: None,
                traits: trait_names,
                fields: Vec::new(),
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
                is_abstract: class_data.is_abstract,
                has_drop: false,
            }),
        );
        self.pre_registered_types.insert(name);
    }

    fn shell_trait(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
        let name_str = name.to_string();
        self.register_type_definition(
            name_str.clone(),
            TypeDefinition::Trait(context::TraitDefinition {
                name: name_str.clone(),
                generics,
                parent_traits: vec![],
                parent_trait_args: BTreeMap::new(),
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
            }),
        );
        self.pre_registered_types.insert(name_str);
    }

    fn shell_struct(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
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

    fn shell_enum(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
        self.register_type_definition(
            name.to_string(),
            TypeDefinition::Enum(context::EnumDefinition {
                variants: BTreeMap::new(),
                generics,
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
                must_use: false,
            }),
        );
    }

    /// Preliminary pass to register declarations without checking their bodies.
    fn collect_declaration(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::FunctionDeclaration(decl) => {
                self.collect_function_decl(decl, context);
            }
            StatementKind::IntrinsicFunctionDeclaration(
                name,
                generics,
                params,
                return_type,
                visibility,
            ) => {
                self.collect_intrinsic_decl(
                    name,
                    generics,
                    params,
                    return_type,
                    visibility,
                    context,
                );
            }
            StatementKind::Class(class_data) => self.collect_class_decl(class_data, context),
            StatementKind::Struct(name_expr, generics_expr, _, _, _) => {
                self.collect_struct_decl(name_expr, generics_expr.as_ref(), context);
            }
            StatementKind::Enum(name_expr, generics_expr, _, _, _, _) => {
                self.collect_enum_decl(name_expr, generics_expr.as_ref(), context);
            }
            StatementKind::Trait(name_expr, generics_expr, _, _, _) => {
                self.collect_trait_decl(name_expr, generics_expr.as_ref(), context);
            }
            StatementKind::Use(path_expr, alias) => {
                self.check_use(path_expr, alias, context);
            }
            _ => {}
        }
    }

    fn collect_function_decl(
        &mut self,
        decl: &crate::ast::statement::FunctionDeclarationData,
        context: &mut Context,
    ) {
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

    fn collect_intrinsic_decl(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        params: &[crate::ast::Parameter],
        return_type: &Option<Box<Expression>>,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
            params: params.to_vec(),
            return_type: return_type.clone(),
        })));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.to_string(),
                SymbolInfo::new_intrinsic(
                    func_type.clone(),
                    visibility.clone(),
                    self.current_module.clone(),
                ),
            );
        }

        context.define(
            name.to_string(),
            SymbolInfo::new_intrinsic(func_type, visibility.clone(), self.current_module.clone()),
        );
    }

    fn collect_class_decl(
        &mut self,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(&class_data.name) else {
            return;
        };
        let is_pre_shell = self.pre_registered_types.contains(name)
            && matches!(
                self.global_type_definitions.get(name),
                Some(TypeDefinition::Class(c)) if c.methods.is_empty()
            );
        if self.global_type_definitions.contains_key(name) && !is_pre_shell {
            return;
        }
        let name = name.to_string();
        let generics = class_data
            .generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));
        let base_class_name: Option<String> = class_data
            .base_class
            .as_ref()
            .and_then(|b| self.extract_type_name(b).ok().map(String::from));
        let trait_names: Vec<String> = class_data
            .traits
            .iter()
            .filter_map(|t| self.extract_type_name(t).ok().map(String::from))
            .collect();

        self.register_class_shell(&name, &generics, &base_class_name, &trait_names, class_data);

        let (methods, base_direct_args) =
            self.scan_class_body(&name, base_class_name.as_deref(), class_data, context);

        let has_drop = methods
            .get("drop")
            .is_some_and(|m| m.params.len() == 1 && m.params[0].0 == "self");
        self.register_type_definition(
            name.clone(),
            TypeDefinition::Class(context::ClassDefinition {
                name,
                generics,
                base_class: base_class_name,
                base_class_args: base_direct_args,
                traits: trait_names,
                fields: Vec::new(),
                methods,
                module: self.current_module.clone(),
                is_abstract: class_data.is_abstract,
                has_drop,
            }),
        );
    }

    fn register_class_shell(
        &mut self,
        name: &str,
        generics: &Option<Vec<context::GenericDefinition>>,
        base_class_name: &Option<String>,
        trait_names: &[String],
        class_data: &crate::ast::statement::ClassData,
    ) {
        self.register_type_definition(
            name.to_string(),
            TypeDefinition::Class(context::ClassDefinition {
                name: name.to_string(),
                generics: generics.clone(),
                base_class: base_class_name.clone(),
                base_class_args: None,
                traits: trait_names.to_vec(),
                fields: Vec::new(),
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
                is_abstract: class_data.is_abstract,
                has_drop: false,
            }),
        );
        self.pre_registered_types.insert(name.to_string());
    }

    fn scan_class_body(
        &mut self,
        name: &str,
        base_class_name: Option<&str>,
        class_data: &crate::ast::statement::ClassData,
        context: &mut Context,
    ) -> (
        BTreeMap<String, context::MethodInfo>,
        Option<Vec<crate::ast::types::Type>>,
    ) {
        context.enter_scope();
        if let Some(gens) = &class_data.generics {
            self.define_generics(gens, context);
        }
        let class_type = make_type(TypeKind::Custom(name.to_string(), None));
        context.enter_class(
            name.to_string(),
            base_class_name.map(String::from),
            class_type,
        );

        let base_direct_args = class_data.base_class.as_ref().and_then(|be| {
            if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) = &be.node {
                Some(
                    args.iter()
                        .map(|arg| self.resolve_type_expression(arg, context))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        });

        let methods = self.collect_class_method_signatures(&class_data.body, context);
        context.exit_class();
        context.exit_scope();
        (methods, base_direct_args)
    }

    fn collect_class_method_signatures(
        &mut self,
        body: &[Statement],
        context: &mut Context,
    ) -> BTreeMap<String, context::MethodInfo> {
        let mut methods = BTreeMap::new();
        for stmt in body {
            let StatementKind::FunctionDeclaration(decl) = &stmt.node else {
                continue;
            };
            let return_ty = if let Some(rt) = &decl.return_type {
                self.resolve_type_expression(rt, context)
            } else {
                make_type(TypeKind::Void)
            };
            let params: Vec<(String, crate::ast::types::Type)> = decl
                .params
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        self.resolve_type_expression(&p.typ, context),
                    )
                })
                .collect();
            let is_out_flags: Vec<bool> = decl.params.iter().map(|p| p.is_out).collect();
            let is_abstract = decl.body.as_ref().is_none_or(|b| {
                matches!(&b.node, StatementKind::Empty)
                    || matches!(&b.node, StatementKind::Block(stmts) if stmts.is_empty())
            });
            methods.insert(
                decl.name.clone(),
                context::MethodInfo {
                    params,
                    is_out_flags,
                    return_type: return_ty,
                    visibility: decl.properties.visibility.clone(),
                    is_constructor: decl.name == "init",
                    is_abstract,
                },
            );
        }
        methods
    }

    fn collect_struct_decl(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
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

    fn collect_enum_decl(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
        self.register_type_definition(
            name.to_string(),
            TypeDefinition::Enum(context::EnumDefinition {
                variants: BTreeMap::new(),
                generics,
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
                must_use: false,
            }),
        );
    }

    fn collect_trait_decl(
        &mut self,
        name_expr: &Expression,
        generics_expr: Option<&Vec<Expression>>,
        context: &mut Context,
    ) {
        let Ok(name) = self.extract_type_name(name_expr) else {
            return;
        };
        if self.global_type_definitions.contains_key(name) {
            return;
        }
        let generics = generics_expr.map(|gens| self.extract_generic_definitions(gens, context));
        let name_str = name.to_string();
        self.register_type_definition(
            name_str.clone(),
            TypeDefinition::Trait(context::TraitDefinition {
                name: name_str.clone(),
                generics,
                parent_traits: vec![],
                parent_trait_args: BTreeMap::new(),
                methods: BTreeMap::new(),
                module: self.current_module.clone(),
            }),
        );
        self.pre_registered_types.insert(name_str);
    }
}
