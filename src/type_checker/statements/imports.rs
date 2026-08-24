// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement type checking for the type checker.
//!
//! This module implements type checking for all statement kinds in Miri.
//! The main entry point is [`TypeChecker::check_statement`], which validates
//! statements and registers type information in the context.
//!
//! # Supported Statements
//!
//! ## Declarations
//! - Variable declarations: `let x = 1`, `var y: int = 2`
//! - Function declarations with generics and return type validation
//! - Struct, enum, class, and trait definitions
//! - Type aliases
//!
//! ## Control Flow
//! - If/else statements with condition type checking
//! - While loops (including forever loops)
//! - For loops with iterator type inference
//! - Match statements with exhaustiveness checking
//! - Return statements with type compatibility validation
//!
//! ## Expressions
//! - Expression statements (side effects)
//! - Assignment validation
//!
//! ## Type Definitions
//! - Structs with fields and generic parameters
//! - Enums with variants and associated values
//! - Classes with fields, methods, and inheritance
//! - Traits with method signatures
//!
//! # Return Type Analysis
//!
//! The module includes return status analysis (`check_returns`) to determine:
//! - Whether all code paths return a value
//! - Implicit vs explicit returns
//! - Return type compatibility

use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

impl TypeChecker {
    pub(crate) fn check_use(
        &mut self,
        path: &Expression,
        alias: &Option<Box<Expression>>,
        context: &mut Context,
    ) {
        // Extract and validate path
        let (path_str, import_kind) = match Self::extract_import_path_with_kind(path) {
            Some(result) => result,
            None => {
                self.report_error(
                    DiagnosticCode::NamImportPathError,
                    "Invalid import path".to_string(),
                    path.span,
                );
                return;
            }
        };

        if path_str.contains("..") || path_str.contains('/') || path_str.contains('\\') {
            self.report_error(
                DiagnosticCode::NamImportPathError,
                "Invalid characters in import path".to_string(),
                path.span,
            );
            return;
        }

        // Resolve file path
        let file_path = match self.resolve_module_path(&path_str, path.span) {
            Some(p) => p,
            None => return,
        };

        // Cycle check
        let abs_path_str = if let Ok(canon) = file_path.canonicalize() {
            canon.to_string_lossy().to_string()
        } else {
            file_path.to_string_lossy().to_string()
        };

        if self.modules.loaded_modules.contains(&abs_path_str) {
            let shadowed = self.modules.shadowed_names_exposed_by(&path_str);
            self.report_shadowed_type_conflicts(&shadowed, &path_str, path.span);
            self.restore_visibility_for_module(&path_str, &import_kind);
            self.replay_module_visibility(&path_str, &import_kind);
            return;
        }

        if self.modules.loading_stack.contains(&abs_path_str) {
            if path_str.starts_with("local.") {
                self.report_circular_import_error(&path_str, &abs_path_str, path.span);
            }
            self.restore_visibility_for_module(&path_str, &import_kind);
            self.replay_module_visibility(&path_str, &import_kind);
            return;
        }

        self.modules.loading_stack.push(abs_path_str.clone());

        // Load and parse module
        let (source, module_ast) =
            match self.load_and_parse_module(&file_path, &path_str, path.span) {
                Some(result) => result,
                None => {
                    self.modules.loading_stack.retain(|m| m != &abs_path_str);
                    return;
                }
            };

        if self.report_shadowed_module_conflicts(&module_ast, &path_str, path.span) {
            self.modules.loading_stack.retain(|m| m != &abs_path_str);
            return;
        }

        let visible_before_load: HashSet<String> = self.type_table.visible_type_names.clone();
        self.process_loaded_module(
            &path_str,
            &file_path,
            &source,
            &module_ast,
            alias,
            context,
            &abs_path_str,
            &import_kind,
            path.span,
        );
        self.record_module_visibility(&path_str, &module_ast, &visible_before_load);

        self.modules.loading_stack.retain(|m| m != &abs_path_str);
        self.modules.loaded_modules.insert(abs_path_str);
    }

    /// Reports each name that user code both declared and imported, returning
    /// whether any conflict was found.
    ///
    /// A program that declares a shadowable prelude type keeps its own, so the
    /// module that would have provided it was never loaded. Importing a module
    /// that needs that name is therefore contradictory: it was written against
    /// the stdlib type. Naming the conflict at the import — before the module is
    /// checked — keeps it to one actionable diagnostic in the program's own file
    /// instead of a cascade of failures inside library source.
    ///
    /// A module conflicts when it declares a shadowed name itself or depends on a
    /// module that provides one.
    fn report_shadowed_module_conflicts(
        &mut self,
        module_ast: &Program,
        path_str: &str,
        span: Span,
    ) -> bool {
        if self.modules.user_shadowed_types.is_empty() {
            return false;
        }

        let mut conflicts: Vec<String> = Vec::new();
        for statement in &module_ast.body {
            match &statement.node {
                StatementKind::Use(dep_path, _) => {
                    conflicts.extend(self.shadowed_names_needed_by(dep_path));
                }
                StatementKind::Class(_)
                | StatementKind::Struct(..)
                | StatementKind::Enum(..)
                | StatementKind::Trait(..) => {
                    if let Some(name) = self.declared_type_name(statement) {
                        if self.modules.user_shadowed_types.contains(&name) {
                            conflicts.push(name);
                        }
                    }
                }
                _ => {}
            }
        }
        conflicts.sort();
        conflicts.dedup();
        self.report_shadowed_type_conflicts(&conflicts, path_str, span)
    }

    /// Shadowed names a dependency brings in: those an already-loaded dependency
    /// exposes (which include the ones its own dependencies contributed) and
    /// those a not-yet-loaded dependency declares itself.
    fn shadowed_names_needed_by(&mut self, dep_path: &Expression) -> Vec<String> {
        let Some((dep_path_str, _)) = Self::extract_import_path_with_kind(dep_path) else {
            return Vec::new();
        };
        let mut names = self.modules.shadowed_names_exposed_by(&dep_path_str);
        if !self.modules.module_visibility.contains_key(&dep_path_str) {
            names.extend(
                self.declared_type_names_of_module(dep_path)
                    .into_iter()
                    .filter(|name| self.modules.user_shadowed_types.contains(name)),
            );
        }
        names
    }

    /// Reports `names` as declared-and-imported conflicts of the module at
    /// `path_str`, returning whether there were any.
    fn report_shadowed_type_conflicts(
        &mut self,
        names: &[String],
        path_str: &str,
        span: Span,
    ) -> bool {
        for name in names {
            self.report_error(
                DiagnosticCode::ImpNameConflict,
                format!(
                    "Type '{}' is declared in this program and also provided by '{}'. \
                     Rename the declaration.",
                    name, path_str
                ),
                span,
            );
        }
        !names.is_empty()
    }

    /// The type name a top-level declaration introduces, if the statement is one.
    fn declared_type_name(&self, statement: &Statement) -> Option<String> {
        let name_expr = match &statement.node {
            StatementKind::Class(class_data) => &class_data.name,
            StatementKind::Struct(name_expr, ..)
            | StatementKind::Enum(name_expr, ..)
            | StatementKind::Trait(name_expr, ..) => name_expr,
            _ => return None,
        };
        self.extract_type_name(name_expr).ok().map(String::from)
    }

    /// Records the full set of type names a module's load exposes, so a later
    /// guarded re-import can replay it (see [`replay_module_visibility`]).
    ///
    /// The set is the names this load *newly* made visible, unioned with the
    /// recorded contributions of every module it directly imports. The union is
    /// essential: a transitive dependency loaded earlier by a sibling import is
    /// not "new" for this module, so a plain visibility diff would miss it (e.g.
    /// `system.ops`'s `Iterable` is loaded before `system.collections.list` during
    /// the implicit-prelude preload, yet must replay when the user imports `list`).
    fn record_module_visibility(
        &mut self,
        path_str: &str,
        module_ast: &Program,
        visible_before_load: &HashSet<String>,
    ) {
        let mut names: HashSet<String> = self
            .type_table
            .visible_type_names
            .difference(visible_before_load)
            .cloned()
            .collect();
        for stmt in &module_ast.body {
            if let StatementKind::Use(dep_path, _) = &stmt.node {
                if let Some((dep_path_str, _)) = Self::extract_import_path_with_kind(dep_path) {
                    if let Some(dep_names) = self.modules.module_visibility.get(&dep_path_str) {
                        names.extend(dep_names.iter().cloned());
                    }
                }
            }
        }
        self.modules
            .module_visibility
            .insert(path_str.to_string(), names.into_iter().collect());
    }

    /// Re-exposes the type names a module's original load made visible when that
    /// module is imported again after being loaded already (e.g. preloaded by the
    /// implicit prelude). Without this, a guarded re-import would restore only the
    /// module's own types and silently drop the transitive ones a fresh load
    /// surfaced. Skipped for selective imports, whose visibility is governed
    /// name-by-name by [`restore_visibility_for_module`].
    fn replay_module_visibility(&mut self, path_str: &str, import_kind: &ImportPathKind) {
        if matches!(import_kind, ImportPathKind::Multi(_)) {
            return;
        }
        if !self.modules.implicitly_preloaded_modules.contains(path_str) {
            return;
        }
        if let Some(names) = self.modules.module_visibility.get(path_str) {
            for name in names {
                self.type_table.visible_type_names.insert(name.clone());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_loaded_module(
        &mut self,
        path_str: &str,
        file_path: &Path,
        source: &str,
        module_ast: &Program,
        alias: &Option<Box<Expression>>,
        context: &mut Context,
        _abs_path_str: &str,
        import_kind: &ImportPathKind,
        span: Span,
    ) {
        let pre_import_globals: HashMap<String, String> = self
            .type_table
            .global_scope
            .iter()
            .map(|(k, v)| (k.clone(), v.module.clone()))
            .collect();
        let pre_import_global_types: HashSet<String> = self
            .type_table
            .global_type_definitions
            .keys()
            .cloned()
            .collect();

        self.type_check_module(path_str, file_path, source, module_ast, alias, context);

        self.restrict_visibility(
            path_str,
            import_kind,
            &pre_import_globals,
            &pre_import_global_types,
            span,
            context,
        );
    }

    fn load_and_parse_module(
        &mut self,
        file_path: &Path,
        path_str: &str,
        span: Span,
    ) -> Option<(String, Program)> {
        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                self.report_error(
                    DiagnosticCode::NamModuleNotFound,
                    format!("Failed to read module '{}': {}", path_str, e),
                    span,
                );
                return None;
            }
        };

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        match parser.parse() {
            Ok(ast) => Some((source, ast)),
            Err(e) => {
                let old_source_override = self.modules.current_source_override.take();
                self.modules.current_source_override =
                    Some((file_path.to_string_lossy().to_string(), source.clone()));
                self.report_syntax_error(&e);
                self.modules.current_source_override = old_source_override;
                None
            }
        }
    }

    fn resolve_module_path(&mut self, path_str: &str, span: Span) -> Option<PathBuf> {
        let current_dir = std::env::current_dir().unwrap_or_default();
        let project_root = self
            .modules
            .source_dir
            .clone()
            .unwrap_or_else(|| current_dir.clone());

        let possible_locations: Vec<(PathBuf, PathBuf)> =
            if let Some(rest) = path_str.strip_prefix("local.") {
                let relative_path = rest.replace('.', "/") + ".mi";
                vec![(project_root.clone(), project_root.join(&relative_path))]
            } else {
                let relative_path = path_str.replace('.', "/") + ".mi";
                let env_override = std::env::var_os("MIRI_STDLIB_PATH").map(PathBuf::from);
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf));
                let mut locations: Vec<(PathBuf, PathBuf)> =
                    stdlib_search_roots(env_override, exe_dir.as_deref())
                        .into_iter()
                        .map(|base| {
                            let loc = base.join(&relative_path);
                            (base, loc)
                        })
                        .collect();
                locations.push((current_dir.clone(), current_dir.join(&relative_path)));
                locations
            };

        for (base, loc) in possible_locations {
            if loc.exists() {
                if let (Ok(canon_loc), Ok(canon_base)) = (loc.canonicalize(), base.canonicalize()) {
                    if canon_loc.starts_with(&canon_base) {
                        return Some(loc);
                    }
                }
            }
        }

        self.report_error(
            DiagnosticCode::NamModuleNotFound,
            format!("Module '{}' not found", path_str),
            span,
        );
        None
    }

    fn report_circular_import_error(&mut self, path_str: &str, abs_path_str: &str, span: Span) {
        let cycle_start = self
            .modules
            .loading_stack
            .iter()
            .position(|m| m == abs_path_str)
            .unwrap_or(0);
        let chain: Vec<&str> = self.modules.loading_stack[cycle_start..]
            .iter()
            .map(|s| s.as_str())
            .collect();
        self.report_error(
            DiagnosticCode::ImpCircularImport,
            format!(
                "Circular import detected: '{}' is already being loaded. Import chain: {} -> {}",
                path_str,
                chain.join(" -> "),
                abs_path_str
            ),
            span,
        );
    }

    fn type_check_module(
        &mut self,
        path_str: &str,
        file_path: &Path,
        source: &str,
        module_ast: &Program,
        alias: &Option<Box<Expression>>,
        context: &mut Context,
    ) {
        let old_module = std::mem::replace(&mut self.modules.current_module, path_str.to_string());
        let old_source_override = self
            .modules
            .current_source_override
            .replace((file_path.to_string_lossy().to_string(), source.to_string()));

        self.module_collect_shells(module_ast);
        self.module_collect_decls(module_ast, context);
        self.module_process_uses(module_ast, context);
        for stmt in &module_ast.body {
            self.check_statement(stmt, context);
        }

        self.modules.current_source_override = old_source_override;
        self.register_module_alias(path_str, alias);
        self.imported_statements.extend(module_ast.body.clone());
        self.modules.current_module = old_module;
    }

    fn module_collect_shells(&mut self, module_ast: &Program) {
        for stmt in &module_ast.body {
            match &stmt.node {
                StatementKind::Use(..) => {}
                StatementKind::Block(stmts) => {
                    for s in stmts {
                        if !matches!(s.node, StatementKind::Use(..)) {
                            self.collect_type_shells(s);
                        }
                    }
                }
                _ => self.collect_type_shells(stmt),
            }
        }
    }

    fn module_collect_decls(&mut self, module_ast: &Program, context: &mut Context) {
        for stmt in &module_ast.body {
            match &stmt.node {
                StatementKind::Use(..) => {}
                StatementKind::Block(stmts) => {
                    for s in stmts {
                        if !matches!(s.node, StatementKind::Use(..)) {
                            self.collect_declaration(s, context);
                        }
                    }
                }
                _ => self.collect_declaration(stmt, context),
            }
        }
    }

    fn module_process_uses(&mut self, module_ast: &Program, context: &mut Context) {
        for stmt in &module_ast.body {
            match &stmt.node {
                StatementKind::Use(..) => self.collect_declaration(stmt, context),
                StatementKind::Block(stmts) => {
                    for s in stmts {
                        if matches!(s.node, StatementKind::Use(..)) {
                            self.collect_declaration(s, context);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn register_module_alias(&mut self, path_str: &str, alias: &Option<Box<Expression>>) {
        if let Some(alias_box) = alias {
            if let ExpressionKind::Identifier(alias_name, _) = &alias_box.node {
                self.modules
                    .module_aliases
                    .insert(alias_name.clone(), path_str.to_string());
            }
        }
    }

    fn restrict_visibility(
        &mut self,
        path_str: &str,
        import_kind: &ImportPathKind,
        pre_import_globals: &HashMap<String, String>,
        pre_import_global_types: &HashSet<String>,
        span: Span,
        context: &mut Context,
    ) {
        let selected_names: Option<HashMap<String, Span>> =
            if let ImportPathKind::Multi(ref items) = import_kind {
                Some(
                    items
                        .iter()
                        .filter_map(|(expr, _alias)| {
                            if let ExpressionKind::Identifier(name, _) = &expr.node {
                                Some((name.clone(), expr.span))
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            } else {
                None
            };

        let module_name = path_str;

        self.detect_namespace_collisions(&selected_names, module_name, pre_import_globals, span);

        let should_be_visible = |name: &str, def_module: Option<&str>| -> bool {
            let is_from_this_module = def_module.is_none_or(|m| m == module_name);
            if !is_from_this_module {
                return false;
            }
            if let Some(ref selected) = selected_names {
                return selected.contains_key(name);
            }
            true
        };

        self.filter_scope_symbols(pre_import_globals, &should_be_visible, context);
        self.filter_type_definitions(pre_import_global_types, module_name, &should_be_visible);
        self.register_item_aliases(import_kind);
        self.validate_selected_exports(&selected_names, module_name, span);
    }

    fn detect_namespace_collisions(
        &mut self,
        selected_names: &Option<HashMap<String, Span>>,
        module_name: &str,
        pre_import_globals: &HashMap<String, String>,
        span: Span,
    ) {
        if let Some(ref selected) = selected_names {
            for sel_name in selected.keys() {
                if let Some(old_module) = pre_import_globals.get(sel_name) {
                    if let Some(info) = self.type_table.global_scope.get(sel_name) {
                        if info.module == module_name {
                            self.report_error(
                                DiagnosticCode::ImpNameConflict,
                                format!(
                                    "Name '{}' conflicts with an existing definition from \
                                     module '{}'. Use selective imports with an alias to \
                                     disambiguate, e.g. `use {}.{{... as ...}}`.",
                                    sel_name, old_module, module_name
                                ),
                                span,
                            );
                        }
                    }
                }
            }
        } else {
            let mut collisions: Vec<(String, String)> = Vec::new();
            for (name, info) in &self.type_table.global_scope {
                if info.module == module_name {
                    if let Some(old_module) = pre_import_globals.get(name) {
                        if old_module != module_name {
                            collisions.push((name.clone(), old_module.clone()));
                        }
                    }
                }
            }
            collisions.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, old_module) in collisions {
                self.report_error(
                    DiagnosticCode::ImpNameConflict,
                    format!(
                        "Name '{}' conflicts with an existing definition from module \
                         '{}'. Use selective imports to avoid ambiguity, e.g. \
                         `use {}.{{...}}`.",
                        name, old_module, module_name
                    ),
                    span,
                );
            }
        }
    }

    fn filter_scope_symbols(
        &mut self,
        pre_import_globals: &HashMap<String, String>,
        should_be_visible: &dyn Fn(&str, Option<&str>) -> bool,
        context: &mut Context,
    ) {
        self.type_table.global_scope.retain(|name, info| {
            if !pre_import_globals.contains_key(name) {
                return should_be_visible(name, Some(info.module.as_str()));
            }
            true
        });

        if let Some(scope) = context.scopes.last_mut() {
            scope.retain(|name, info| {
                if !pre_import_globals.contains_key(name) {
                    return should_be_visible(name, Some(info.module.as_str()));
                }
                true
            });
        }
    }

    fn filter_type_definitions(
        &mut self,
        pre_import_global_types: &HashSet<String>,
        module_name: &str,
        should_be_visible: &dyn Fn(&str, Option<&str>) -> bool,
    ) {
        self.type_table.global_type_definitions.retain(|name, def| {
            if !pre_import_global_types.contains(name) {
                let def_module = match def {
                    TypeDefinition::Class(cd) => Some(cd.module.as_str()),
                    TypeDefinition::Trait(td) => Some(td.module.as_str()),
                    TypeDefinition::Struct(sd) => Some(sd.module.as_str()),
                    TypeDefinition::Enum(ed) => Some(ed.module.as_str()),
                    _ => None,
                };
                if should_be_visible(name, def_module) {
                    return true;
                }
                let is_transitive = def_module.is_some_and(|m| m != module_name);
                if is_transitive {
                    self.type_table.visible_type_names.remove(name);
                    return true;
                }
                self.type_table.visible_type_names.remove(name);
                return false;
            }
            true
        });
    }

    fn register_item_aliases(&mut self, import_kind: &ImportPathKind) {
        if let ImportPathKind::Multi(ref items) = import_kind {
            for (name_expr, item_alias_opt) in items {
                if let ExpressionKind::Identifier(orig_name, _) = &name_expr.node {
                    if let Some(alias_box) = item_alias_opt {
                        if let ExpressionKind::Identifier(alias_name, _) = &alias_box.node {
                            if let Some(info) = self.type_table.global_scope.get(orig_name).cloned()
                            {
                                let mut aliased = info;
                                aliased.original_name = Some(orig_name.clone());
                                self.type_table
                                    .global_scope
                                    .insert(alias_name.clone(), aliased);
                            }
                        }
                    }
                }
            }
        }
    }

    fn validate_selected_exports(
        &mut self,
        selected_names: &Option<HashMap<String, Span>>,
        module_name: &str,
        _span: Span,
    ) {
        if let Some(ref selected) = selected_names {
            for (sel_name, sel_span) in selected {
                let in_scope = self
                    .type_table
                    .global_scope
                    .get(sel_name.as_str())
                    .is_some_and(|info| info.module == module_name);

                let in_types = self
                    .type_table
                    .global_type_definitions
                    .get(sel_name.as_str())
                    .is_some_and(|def| {
                        let def_module = match def {
                            TypeDefinition::Class(cd) => Some(cd.module.as_str()),
                            TypeDefinition::Trait(td) => Some(td.module.as_str()),
                            TypeDefinition::Struct(sd) => Some(sd.module.as_str()),
                            TypeDefinition::Enum(ed) => Some(ed.module.as_str()),
                            _ => None,
                        };
                        def_module == Some(module_name)
                    })
                    && self
                        .type_table
                        .visible_type_names
                        .contains(sel_name.as_str());

                if !in_scope && !in_types {
                    self.report_error(
                        DiagnosticCode::ImpNameNotFoundInModule,
                        format!("Name '{}' not found in module '{}'", sel_name, module_name),
                        *sel_span,
                    );
                }
            }
        }
    }

    /// Restores visibility for types defined in an already-loaded module.
    ///
    /// When a module M is first loaded by module A, M's types become visible.
    /// A's post-import filter may then hide them (they're transitive to A).
    /// If module B later imports M directly, this method makes M's types
    /// visible again without re-parsing or re-type-checking M.
    fn restore_visibility_for_module(&mut self, module_path: &str, import_kind: &ImportPathKind) {
        let selected_names = Self::extract_selected_names(import_kind);
        for (name, def) in &self.type_table.global_type_definitions {
            if self.should_restore_visibility(name, def, module_path, &selected_names) {
                self.type_table.visible_type_names.insert(name.clone());
            }
        }
    }

    fn extract_selected_names(import_kind: &ImportPathKind) -> Option<HashSet<String>> {
        if let ImportPathKind::Multi(ref items) = import_kind {
            Some(
                items
                    .iter()
                    .filter_map(|(expr, _alias)| {
                        if let ExpressionKind::Identifier(name, _) = &expr.node {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        } else {
            None
        }
    }

    fn should_restore_visibility(
        &self,
        name: &str,
        def: &TypeDefinition,
        module_path: &str,
        selected_names: &Option<HashSet<String>>,
    ) -> bool {
        let def_module = match def {
            TypeDefinition::Class(cd) => Some(cd.module.as_str()),
            TypeDefinition::Trait(td) => Some(td.module.as_str()),
            TypeDefinition::Struct(sd) => Some(sd.module.as_str()),
            TypeDefinition::Enum(ed) => Some(ed.module.as_str()),
            _ => None,
        };
        if def_module != Some(module_path) {
            return false;
        }
        if let Some(ref selected) = selected_names {
            selected.contains(name)
        } else {
            true
        }
    }

    /// Extracts the module path string and import kind from a use-statement expression.
    ///
    /// For `use system.io.{println}`, returns `("system.io", Multi([...]))`.
    /// For `use system.io`, returns `("system.io", Simple)`.
    pub(crate) fn extract_import_path_with_kind(
        expr: &Expression,
    ) -> Option<(String, ImportPathKind)> {
        match &expr.node {
            ExpressionKind::ImportPath(segments, kind) => {
                let parts: Vec<String> = segments
                    .iter()
                    .filter_map(|s| {
                        if let ExpressionKind::Identifier(n, _) = &s.node {
                            Some(n.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                Some((parts.join("."), kind.clone()))
            }
            ExpressionKind::Identifier(name, _) => Some((name.clone(), ImportPathKind::Simple)),
            ExpressionKind::Member(obj, member) => {
                let (parent, kind) = Self::extract_import_path_with_kind(obj)?;
                let member_name = if let ExpressionKind::Identifier(n, _) = &member.node {
                    n
                } else {
                    return None;
                };
                Some((format!("{}.{}", parent, member_name), kind))
            }
            _ => None,
        }
    }

    /// Loads the implicit prelude before the user's code is type-checked.
    ///
    /// There are three tiers, all sourced from stdlib so the compiler hardcodes
    /// no stdlib type or module names:
    ///
    /// - `system/prelude.mi` — re-exported by name: its modules' symbols (e.g.
    ///   `println`, `String`) are available unqualified, mirroring Rust's
    ///   `std::prelude`.
    /// - `system/prelude_internal.mi` — loaded for definitions only: backing
    ///   classes for collection literals (`Array`/`List`/`Map`/`Set`) are needed
    ///   so a `[1, 2, 3]` literal can resolve methods and be gpu-resident, but the
    ///   user has not named them, so writing `Array<…>(…)` must still require an
    ///   explicit `use system.collections.array`. Their names are dropped from
    ///   `visible_type_names` after loading; only the definitions remain.
    /// - `system/prelude_shadowable.mi` — re-exported by name like the first
    ///   tier, but skipped for any module whose types the program declares
    ///   itself. Loaded separately by [`load_shadowable_prelude`] after the
    ///   program's own type names are known.
    ///
    /// Loading happens at clean type-checker state (before any user expression),
    /// which is required: the collection modules pull a trait web whose default
    /// methods only resolve correctly outside an in-progress inference.
    ///
    /// Missing files are a silent no-op so isolated tests without stdlib still
    /// compile. The already-loaded guard in [`check_use`] keeps an explicit
    /// `use system.string` in user code a no-op.
    pub(crate) fn load_prelude(&mut self, context: &mut Context) {
        self.load_prelude_file("prelude.mi", context);

        let visible_before = self.type_table.visible_type_names.clone();
        let modules_before: HashSet<String> =
            self.modules.module_visibility.keys().cloned().collect();
        self.load_prelude_file("prelude_internal.mi", context);
        self.type_table.visible_type_names = visible_before;

        // Every module loaded while processing the internal prelude — the listed
        // collection modules AND their transitive deps (queryable, ops, …) — is
        // marked preloaded, so a later explicit `use` of any of them replays its
        // full visibility (an explicit `use system.collections.queryable` must
        // still expose the transitive `Iterable` it would on a fresh load).
        let newly_loaded: Vec<String> = self
            .modules
            .module_visibility
            .keys()
            .filter(|module| !modules_before.contains(*module))
            .cloned()
            .collect();
        self.modules
            .implicitly_preloaded_modules
            .extend(newly_loaded);
    }

    /// Loads the shadowable prelude tier, skipping any module whose types the
    /// program declares itself.
    ///
    /// Runs after the type-shell pass, so `global_type_definitions` already holds
    /// the program's own declarations and a collision is visible before the module
    /// is loaded. Skipping rather than replacing is what makes the tier safe: a
    /// loaded module's methods are compiled against the definitions it was checked
    /// with, so replacing one of its types afterwards would leave that code
    /// building values of a layout that no longer exists.
    ///
    /// A skipped name is recorded in `user_shadowed_types` so that importing a
    /// module which needs it — the module the program shadowed, or any module
    /// written against it — reports the conflict instead of failing inside
    /// library source.
    pub(crate) fn load_shadowable_prelude(&mut self, context: &mut Context) {
        let Some(imports) = Self::prelude_file_imports("prelude_shadowable.mi") else {
            return;
        };

        for (path_expr, _) in imports {
            let declared = self.declared_type_names_of_module(&path_expr);
            let shadowed: Vec<String> = declared
                .into_iter()
                .filter(|name| self.type_table.global_type_definitions.contains_key(name))
                .collect();

            if shadowed.is_empty() {
                self.check_use(&path_expr, &None, context);
            } else {
                self.modules.user_shadowed_types.extend(shadowed);
            }
        }
    }

    /// The type names a module declares at its top level, without type-checking
    /// it. Parsing alone is enough to see the names, and it must stay that way:
    /// the module is not loaded when the program shadows one of them.
    fn declared_type_names_of_module(&mut self, path_expr: &Expression) -> Vec<String> {
        let Some((path_str, _)) = Self::extract_import_path_with_kind(path_expr) else {
            return Vec::new();
        };
        let Some(file_path) = self.resolve_module_path(&path_str, path_expr.span) else {
            return Vec::new();
        };
        let Some((_, module_ast)) =
            self.load_and_parse_module(&file_path, &path_str, path_expr.span)
        else {
            return Vec::new();
        };
        module_ast
            .body
            .iter()
            .filter_map(|statement| self.declared_type_name(statement))
            .collect()
    }

    /// Parses one stdlib prelude file under `system/` and runs each of its
    /// top-level `use` statements as a normal import. Loading them at top level
    /// (rather than nested under a single synthetic import) keeps each module's
    /// own symbols past the visibility filter.
    fn load_prelude_file(&mut self, file_name: &str, context: &mut Context) {
        let Some(imports) = Self::prelude_file_imports(file_name) else {
            return;
        };
        for (path_expr, alias_expr) in imports {
            self.check_use(&path_expr, &alias_expr, context);
        }
    }

    /// The imports a prelude file lists, or `None` when the file is absent or
    /// unparseable — a silent no-op that keeps isolated tests without a stdlib
    /// tree compiling.
    #[allow(clippy::type_complexity)]
    fn prelude_file_imports(
        file_name: &str,
    ) -> Option<Vec<(Box<Expression>, Option<Box<Expression>>)>> {
        let stdlib_base = std::env::var("MIRI_STDLIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("src/stdlib"));

        let file_path = stdlib_base.join("system").join(file_name);
        let source = fs::read_to_string(file_path).ok()?;

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        let ast = parser.parse().ok()?;

        Some(
            ast.body
                .iter()
                .filter_map(|stmt| match &stmt.node {
                    StatementKind::Use(path_expr, alias_expr) => {
                        Some((path_expr.clone(), alias_expr.clone()))
                    }
                    _ => None,
                })
                .collect(),
        )
    }

    /// Returns the stdlib module path that defines `type_name`, or `None` if
    /// the type is not found in the stdlib directory.
    ///
    /// This is used to generate actionable import hints in error messages (e.g.
    /// "Consider importing 'system.collections.array'") without hard-coding any
    /// stdlib module paths in the compiler source.  The scan is intentionally
    /// lazy — it runs only on error paths — so its cost is not felt in the
    /// normal (success) compilation path.
    pub(crate) fn suggest_module_for_type(&self, type_name: &str) -> Option<String> {
        let stdlib_base = std::env::var("MIRI_STDLIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("src/stdlib"));

        Self::scan_dir_for_class_definition(&stdlib_base, type_name, &stdlib_base)
    }

    /// Emits a unified "unknown type, consider importing" diagnostic when
    /// `type_name` names a stdlib type that exists but is not imported into the
    /// current scope. Returns `true` if the hint was emitted, signalling the
    /// caller to suppress its own fallback error so the two "named hidden type"
    /// paths (the bare collection identifier and the sized-array constructor)
    /// surface the same actionable message.
    pub(crate) fn report_hidden_type_import_hint(&mut self, type_name: &str, span: Span) -> bool {
        match self.suggest_module_for_type(type_name) {
            Some(module) => {
                self.report_error_with_help(
                    DiagnosticCode::TypTypeNotFound,
                    format!("Unknown type: {}", type_name),
                    span,
                    format!("Consider importing '{}'", module),
                );
                true
            }
            None => false,
        }
    }

    /// Recursively scans `dir` for a `.mi` file whose top-level declarations
    /// include `class <type_name>`.  Returns the dot-separated module path
    /// (e.g. `"system.collections.array"`) derived from the file's position
    /// relative to `base`, or `None` if no such file is found.
    fn scan_dir_for_class_definition(dir: &Path, type_name: &str, base: &Path) -> Option<String> {
        let read_dir = fs::read_dir(dir).ok()?;

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(result) = Self::scan_dir_for_class_definition(&path, type_name, base) {
                    return Some(result);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("mi") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let defines_type = content.lines().any(|line| {
                        let trimmed = line.trim();
                        // Skip comment lines.
                        if trimmed.starts_with("//") {
                            return false;
                        }
                        // Look for `class <type_name>` as adjacent whitespace-separated tokens,
                        // handling optional modifiers like `public` or `abstract`, and
                        // stripping any generic parameters (e.g. `Array<T, Size>` → `Array`).
                        trimmed
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .windows(2)
                            .any(|w| {
                                w[0] == "class"
                                    && w[1].split('<').next().unwrap_or(w[1]) == type_name
                            })
                    });

                    if defines_type {
                        if let Ok(relative) = path.strip_prefix(base) {
                            let parts: Vec<String> = relative
                                .components()
                                .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
                                .collect();
                            if let Some(last_part) = parts.last() {
                                let last = last_part.trim_end_matches(".mi").to_string();
                                let mut module_parts = parts[..parts.len() - 1].to_vec();
                                module_parts.push(last);
                                return Some(module_parts.join("."));
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

/// Ordered stdlib search roots, highest priority first, so the compiler
/// locates `system.*` modules whether it runs from the repo root during
/// development or as an installed binary invoked from an arbitrary directory.
///
/// The first existing root that contains the requested module wins:
/// 1. `MIRI_STDLIB_PATH` override, when set.
/// 2. `src/stdlib` relative to the current directory (in-repo development).
/// 3. Roots derived from the compiler binary's own location, covering both a
///    binary shipped with a sibling `stdlib/` directory and the common
///    `<prefix>/bin/miri` + `<prefix>/{lib,share}/miri/stdlib` install layouts.
///
/// Deriving from the binary location (rather than the current directory) is
/// what lets an installed `miri` resolve the stdlib from any working directory.
fn stdlib_search_roots(env_override: Option<PathBuf>, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(base) = env_override {
        roots.push(base);
    }
    roots.push(PathBuf::from("src/stdlib"));
    if let Some(dir) = exe_dir {
        roots.push(dir.join("stdlib"));
        if let Some(prefix) = dir.parent() {
            roots.push(prefix.join("lib").join("miri").join("stdlib"));
            roots.push(prefix.join("share").join("miri").join("stdlib"));
        }
    }
    roots
}

#[cfg(test)]
mod stdlib_search_root_tests {
    use super::stdlib_search_roots;
    use std::path::{Path, PathBuf};

    #[test]
    fn repo_relative_root_present_without_env_or_exe() {
        let roots = stdlib_search_roots(None, None);
        assert_eq!(roots, vec![PathBuf::from("src/stdlib")]);
    }

    #[test]
    fn env_override_takes_priority() {
        let roots = stdlib_search_roots(Some(PathBuf::from("/custom/stdlib")), None);
        assert_eq!(roots.first(), Some(&PathBuf::from("/custom/stdlib")));
        // The repo-relative fallback is still appended after the override.
        assert!(roots.contains(&PathBuf::from("src/stdlib")));
    }

    #[test]
    fn exe_dir_yields_sibling_and_install_prefix_roots() {
        let roots = stdlib_search_roots(None, Some(Path::new("/opt/miri/bin")));
        assert!(roots.contains(&PathBuf::from("/opt/miri/bin/stdlib")));
        assert!(roots.contains(&PathBuf::from("/opt/miri/lib/miri/stdlib")));
        assert!(roots.contains(&PathBuf::from("/opt/miri/share/miri/stdlib")));
    }

    #[test]
    fn priority_order_env_then_repo_then_install() {
        let roots = stdlib_search_roots(
            Some(PathBuf::from("/env/stdlib")),
            Some(Path::new("/opt/miri/bin")),
        );
        assert_eq!(
            roots,
            vec![
                PathBuf::from("/env/stdlib"),
                PathBuf::from("src/stdlib"),
                PathBuf::from("/opt/miri/bin/stdlib"),
                PathBuf::from("/opt/miri/lib/miri/stdlib"),
                PathBuf::from("/opt/miri/share/miri/stdlib"),
            ]
        );
    }

    #[test]
    fn exe_dir_at_filesystem_root_has_no_prefix_roots() {
        // A binary directly at `/` has no parent prefix; only the sibling
        // `stdlib` root is derivable, never a panic.
        let roots = stdlib_search_roots(None, Some(Path::new("/")));
        assert!(roots.contains(&PathBuf::from("/stdlib")));
        assert!(!roots.iter().any(|r| r.ends_with("lib/miri/stdlib")));
    }
}
