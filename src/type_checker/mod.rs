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

        // Load the implicit prelude so that core types (e.g. String) are available
        // in every program without an explicit `use` statement.
        self.load_prelude(&mut context);

        // Pass 1a: Register class/trait/struct/enum NAMES (shells only). This
        // makes every top-level type identifier visible to any other top-level
        // declaration's method-signature resolution in pass 1b.
        for statement in &program.body {
            if let StatementKind::Block(stmts) = &statement.node {
                for stmt in stmts {
                    self.collect_type_shells(stmt);
                }
            } else {
                self.collect_type_shells(statement);
            }
        }

        // Pass 1b: Collect full top-level declarations — function signatures
        // and class/trait method signatures. Forward references between top-level
        // types are now resolvable because every type is at least a shell.
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

        // Pass 2.5: compute escape summaries for all user-defined functions.
        // Must run after type checking (we need the `self.types` map) and
        // before use-after-move (which consults the summaries).
        if self.errors.is_empty() {
            let ffi_summaries = std::mem::take(&mut context.escape_summaries);
            context.escape_summaries = escape_analysis::compute_escape_summaries(
                &program.body,
                &self.types,
                &self.global_type_definitions,
                ffi_summaries,
            );
        }

        // Pass 3: use-after-move analysis — runs only when type checking is clean
        // so that we don't emit spurious "consumed" errors on top of type errors.
        if self.errors.is_empty() {
            let (uam_errors, uam_warnings) = use_after_move::UseAfterMoveChecker::new(
                &self.types,
                &self.global_type_definitions,
                &context.escape_summaries,
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

    /// Phase 1a — register class/trait/struct/enum names as empty shells.
    /// Runs before `collect_declaration` so that method signatures in later
    /// declarations can resolve forward references to types declared later
    /// in the same module (or in a module that recursively imports us back).
    pub(crate) fn collect_type_shells(&mut self, statement: &Statement) {
        let mut context = Context::new();
        let context = &mut context;
        match &statement.node {
            StatementKind::Class(class_data) => {
                if let Ok(name) = self.extract_type_name(&class_data.name) {
                    if !self.global_type_definitions.contains_key(name) {
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
                            name.to_string(),
                            TypeDefinition::Class(context::ClassDefinition {
                                name: name.to_string(),
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
                        self.pre_registered_types.insert(name.to_string());
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
                                parent_trait_args: BTreeMap::new(),
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                            }),
                        );
                        self.pre_registered_types.insert(name.to_string());
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
            StatementKind::Enum(name_expr, generics_expr, _, _, _, _) => {
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
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                                must_use: false,
                            }),
                        );
                    }
                }
            }
            _ => {}
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
            StatementKind::IntrinsicFunctionDeclaration(
                name,
                generics,
                params,
                return_type,
                visibility,
            ) => {
                let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: generics.clone(),
                    params: params.to_vec(),
                    return_type: return_type.clone(),
                })));

                if context.scopes.len() == 1 {
                    self.global_scope.insert(
                        name.clone(),
                        SymbolInfo::new_intrinsic(
                            func_type.clone(),
                            visibility.clone(),
                            self.current_module.clone(),
                        ),
                    );
                }

                context.define(
                    name.clone(),
                    SymbolInfo::new_intrinsic(
                        func_type,
                        visibility.clone(),
                        self.current_module.clone(),
                    ),
                );
            }
            StatementKind::Class(class_data) => {
                // Register class name AND method signatures in global_type_definitions
                // so cross-module forward references resolve method names too. Without
                // method signatures here, a trait default body in another module would
                // see `List` registered but `List.push` invisible until the full
                // class-check runs. Field types and method bodies are deferred to
                // the body-check pass. We also extract trait names from the
                // `implements` clause so trait-default-method lookup works against
                // this class during cross-module body checks; full validation of
                // those traits (existence, signatures matching) happens in
                // `check_class` later.
                if let Ok(name) = self.extract_type_name(&class_data.name) {
                    // Allow overwriting a pre-registered shell (from
                    // `collect_type_shells`). A real duplicate would still error
                    // later in `check_class`.
                    let is_pre_shell = self.pre_registered_types.contains(name)
                        && matches!(
                            self.global_type_definitions.get(name),
                            Some(TypeDefinition::Class(c)) if c.methods.is_empty()
                        );
                    if !self.global_type_definitions.contains_key(name) || is_pre_shell {
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

                        // STEP 1: register the bare class shell so the class's own
                        // name resolves inside its method signatures (e.g.
                        // `fn clone() Point` inside `class Point`).
                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Class(context::ClassDefinition {
                                name: name.to_string(),
                                generics: generics.clone(),
                                base_class: base_class_name.clone(),
                                base_class_args: None,
                                traits: trait_names.clone(),
                                fields: Vec::new(),
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                                is_abstract: class_data.is_abstract,
                                has_drop: false,
                            }),
                        );
                        self.pre_registered_types.insert(name.to_string());

                        // STEP 2: extract method signatures (params + return type,
                        // no body checks). Enter class scope so `Self` resolves
                        // inside method signatures (e.g. `fn drop(self)` parameter
                        // type), and define class generics so `T` in `List<T>`
                        // resolves while we resolve method-param/return types.
                        context.enter_scope();
                        if let Some(gens) = &class_data.generics {
                            self.define_generics(gens, context);
                        }
                        let class_type = make_type(TypeKind::Custom(name.to_string(), None));
                        context.enter_class(name.to_string(), base_class_name.clone(), class_type);

                        // Resolve `extends Base<...>` args in this class's
                        // generic-param scope so descendants whose check_class
                        // runs before ours can compose substitutions through
                        // this ancestor. Without this, intermediate generic
                        // ancestors look unparameterized at child-check time.
                        let base_direct_args: Option<Vec<crate::ast::types::Type>> =
                            class_data.base_class.as_ref().and_then(|be| {
                                if let ExpressionKind::TypeDeclaration(_, Some(args), _, _) =
                                    &be.node
                                {
                                    Some(
                                        args.iter()
                                            .map(|arg| self.resolve_type_expression(arg, context))
                                            .collect(),
                                    )
                                } else {
                                    None
                                }
                            });

                        let mut methods: BTreeMap<String, context::MethodInfo> = BTreeMap::new();
                        for stmt in &class_data.body {
                            if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
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
                                let is_abstract = decl.body.as_ref().is_none_or(|b| {
                                    matches!(&b.node, StatementKind::Empty)
                                        || matches!(
                                            &b.node,
                                            StatementKind::Block(stmts) if stmts.is_empty()
                                        )
                                });
                                methods.insert(
                                    decl.name.clone(),
                                    context::MethodInfo {
                                        params,
                                        return_type: return_ty,
                                        visibility: decl.properties.visibility.clone(),
                                        is_constructor: decl.name == "init",
                                        is_abstract,
                                    },
                                );
                            }
                        }
                        context.exit_class();
                        context.exit_scope();

                        // STEP 3: overwrite the bare shell with the version that
                        // carries method signatures. Compute `has_drop` here so
                        // resource-bound detection works even when other passes
                        // consult the type before `check_class` re-registers it.
                        let has_drop = methods
                            .get("drop")
                            .is_some_and(|m| m.params.len() == 1 && m.params[0].0 == "self");
                        self.register_type_definition(
                            name.to_string(),
                            TypeDefinition::Class(context::ClassDefinition {
                                name: name.to_string(),
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
            StatementKind::Enum(name_expr, generics_expr, _, _, _, _) => {
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
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                                must_use: false,
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
                                parent_trait_args: BTreeMap::new(),
                                methods: BTreeMap::new(),
                                module: self.current_module.clone(),
                            }),
                        );
                        self.pre_registered_types.insert(name.to_string());
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
