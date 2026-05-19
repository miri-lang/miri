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

use crate::ast::factory;
use crate::ast::*;
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
                self.report_error("Invalid import path".to_string(), path.span);
                return;
            }
        };

        if path_str.contains("..") || path_str.contains('/') || path_str.contains('\\') {
            self.report_error("Invalid characters in import path".to_string(), path.span);
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

        if self.loaded_modules.contains(&abs_path_str) {
            self.restore_visibility_for_module(&path_str, &import_kind);
            return;
        }

        if self.loading_stack.contains(&abs_path_str) {
            if path_str.starts_with("local.") {
                self.report_circular_import_error(&path_str, &abs_path_str, path.span);
            }
            self.restore_visibility_for_module(&path_str, &import_kind);
            return;
        }

        self.loading_stack.push(abs_path_str.clone());

        // Load and parse module
        let (source, module_ast) =
            match self.load_and_parse_module(&file_path, &path_str, path.span) {
                Some(result) => result,
                None => {
                    self.loading_stack.retain(|m| m != &abs_path_str);
                    return;
                }
            };

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

        self.loading_stack.retain(|m| m != &abs_path_str);
        self.loaded_modules.insert(abs_path_str);
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
            .global_scope
            .iter()
            .map(|(k, v)| (k.clone(), v.module.clone()))
            .collect();
        let pre_import_global_types: HashSet<String> =
            self.global_type_definitions.keys().cloned().collect();

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
                self.report_error(format!("Failed to read module '{}': {}", path_str, e), span);
                return None;
            }
        };

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        match parser.parse() {
            Ok(ast) => Some((source, ast)),
            Err(e) => {
                let old_source_override = self.current_source_override.take();
                self.current_source_override =
                    Some((file_path.to_string_lossy().to_string(), source.clone()));
                self.report_syntax_error(&e);
                self.current_source_override = old_source_override;
                None
            }
        }
    }

    fn resolve_module_path(&mut self, path_str: &str, span: Span) -> Option<PathBuf> {
        let stdlib_base = std::env::var("MIRI_STDLIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("src/stdlib"));

        let current_dir = std::env::current_dir().unwrap_or_default();
        let project_root = self
            .source_dir
            .clone()
            .unwrap_or_else(|| current_dir.clone());

        let possible_locations: Vec<(PathBuf, PathBuf)> =
            if let Some(rest) = path_str.strip_prefix("local.") {
                let relative_path = rest.replace('.', "/") + ".mi";
                vec![(project_root.clone(), project_root.join(&relative_path))]
            } else {
                let relative_path = path_str.replace('.', "/") + ".mi";
                vec![
                    (stdlib_base.clone(), stdlib_base.join(&relative_path)),
                    (current_dir.clone(), current_dir.join(&relative_path)),
                ]
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

        self.report_error(format!("Module '{}' not found", path_str), span);
        None
    }

    fn report_circular_import_error(&mut self, path_str: &str, abs_path_str: &str, span: Span) {
        let cycle_start = self
            .loading_stack
            .iter()
            .position(|m| m == abs_path_str)
            .unwrap_or(0);
        let chain: Vec<&str> = self.loading_stack[cycle_start..]
            .iter()
            .map(|s| s.as_str())
            .collect();
        self.report_error(
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
        let old_module = std::mem::replace(&mut self.current_module, path_str.to_string());
        let old_source_override = self
            .current_source_override
            .replace((file_path.to_string_lossy().to_string(), source.to_string()));

        self.module_collect_shells(module_ast);
        self.module_collect_decls(module_ast, context);
        self.module_process_uses(module_ast, context);
        for stmt in &module_ast.body {
            self.check_statement(stmt, context);
        }

        self.current_source_override = old_source_override;
        self.register_module_alias(path_str, alias);
        self.imported_statements.extend(module_ast.body.clone());
        self.current_module = old_module;
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
                self.module_aliases
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
                    if let Some(info) = self.global_scope.get(sel_name) {
                        if info.module == module_name {
                            self.report_error(
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
            for (name, info) in &self.global_scope {
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
        self.global_scope.retain(|name, info| {
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
        self.global_type_definitions.retain(|name, def| {
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
                    self.visible_type_names.remove(name);
                    return true;
                }
                self.visible_type_names.remove(name);
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
                            if let Some(info) = self.global_scope.get(orig_name).cloned() {
                                let mut aliased = info;
                                aliased.original_name = Some(orig_name.clone());
                                self.global_scope.insert(alias_name.clone(), aliased);
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
                    .global_scope
                    .get(sel_name.as_str())
                    .is_some_and(|info| info.module == module_name);

                let in_types = self
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
                    && self.visible_type_names.contains(sel_name.as_str());

                if !in_scope && !in_types {
                    self.report_error(
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
        for (name, def) in &self.global_type_definitions {
            if self.should_restore_visibility(name, def, module_path, &selected_names) {
                self.visible_type_names.insert(name.clone());
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

    /// Loads the implicit prelude, making its definitions available in every program.
    ///
    /// The prelude (`system/prelude.mi`) is loaded exactly once before the user's
    /// code is type-checked. If the prelude file cannot be found (e.g. in isolated
    /// test environments without stdlib), this is a silent no-op — programs still
    /// compile but without implicit prelude imports.
    ///
    /// The already-loaded guard in [`check_use`] ensures that an explicit
    /// `use system.string` (or any other prelude module) in user code is a no-op.
    pub(crate) fn load_prelude(&mut self, context: &mut Context) {
        let stdlib_base = std::env::var("MIRI_STDLIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("src/stdlib"));

        // Only attempt the load when the file physically exists; silently skip
        // otherwise so that isolated unit tests without stdlib still work.
        if !stdlib_base.join("system").join("prelude.mi").exists() {
            return;
        }

        let path = factory::expr_with_span(
            ExpressionKind::ImportPath(
                vec![
                    factory::identifier_with_span("system", Span::default()),
                    factory::identifier_with_span("prelude", Span::default()),
                ],
                ImportPathKind::Simple,
            ),
            Span::default(),
        );
        // The prelude's purpose is to make its imports globally available.
        // Snapshot visible types before, load the prelude, then ensure
        // everything it made visible stays visible (the normal post-import
        // filter would hide transitive types, but the prelude exists
        // precisely to re-export them).
        let pre = self.visible_type_names.clone();
        self.check_use(&path, &None, context);
        // Re-add any types that were visible during prelude loading but
        // hidden by the post-import filter.
        for name in self.global_type_definitions.keys() {
            if !pre.contains(name) && !self.visible_type_names.contains(name) {
                self.visible_type_names.insert(name.clone());
            }
        }
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
