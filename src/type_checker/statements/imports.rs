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
        // 1. Extract path string and import kind
        let (path_str, import_kind) = match Self::extract_import_path_with_kind(path) {
            Some(result) => result,
            None => {
                self.report_error("Invalid import path".to_string(), path.span);
                return;
            }
        };

        // 1.5 Security: Sanitize path string to prevent traversal
        if path_str.contains("..") || path_str.contains('/') || path_str.contains('\\') {
            self.report_error("Invalid characters in import path".to_string(), path.span);
            return;
        }

        // 2. Resolve file path.
        //
        // `local.*` is the project-local namespace:  `local.utils.math` maps to
        // `utils/math.mi` relative to the project root (= the source_dir of the
        // entry-point file).  All other imports are resolved against stdlib first,
        // then the current working directory.
        //
        // The stdlib location can be overridden via the `MIRI_STDLIB_PATH`
        // environment variable so that integration tests can change the working
        // directory without losing access to the standard library.
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
                // `local.*` — look only inside the project root.
                let relative_path = rest.replace('.', "/") + ".mi";
                vec![(project_root.clone(), project_root.join(&relative_path))]
            } else {
                // stdlib + current working directory (existing behaviour).
                let relative_path = path_str.replace('.', "/") + ".mi";
                vec![
                    (stdlib_base.clone(), stdlib_base.join(&relative_path)),
                    (current_dir.clone(), current_dir.join(&relative_path)),
                ]
            };

        let mut found_path = None;
        for (base, loc) in possible_locations {
            // Security: Prevent path traversal by ensuring the resolved
            // path is physically inside the intended base directory.
            // Using components ensures we catch "foo/../bar" correctly.
            // But a simpler check is whether it's syntactically within
            // or we can canonicalize. Since we only append ".replace('.', '/') + .mi"
            // and identifiers shouldn't have "..", this is defense in depth.
            // Note: `starts_with` only does syntactic path checking, but since
            // relative_path doesn't start with `/` and base is absolute/relative,
            // we should normalize or canonicalize to be perfectly safe, or just
            // use the standard path sanitization pattern.

            // To properly prevent traversal like `base.join("..").join("etc")`,
            // we check if canonicalized paths align, or at least if `loc.starts_with`
            // works on the parsed PathBuf. Since PathBuf::join resolves `..` sometimes
            // or just concatenates, we should check canonical paths if exists.
            if loc.exists() {
                if let (Ok(canon_loc), Ok(canon_base)) = (loc.canonicalize(), base.canonicalize()) {
                    if canon_loc.starts_with(&canon_base) {
                        found_path = Some(loc);
                        break;
                    }
                }
            }
        }

        let file_path = match found_path {
            Some(p) => p,
            None => {
                self.report_error(format!("Module '{}' not found", path_str), path.span);
                return;
            }
        };

        // 3. Cycle check
        let abs_path_str = if let Ok(canon) = file_path.canonicalize() {
            canon.to_string_lossy().to_string()
        } else {
            file_path.to_string_lossy().to_string()
        };

        if self.loaded_modules.contains(&abs_path_str) {
            // Module already loaded — skip parsing/type-checking but restore
            // visibility for types defined in this module.  A previous import
            // may have hidden them (e.g. they were transitive to that importer).
            self.restore_visibility_for_module(&path_str, &import_kind);
            return;
        }

        if self.loading_stack.contains(&abs_path_str) {
            // Build chain: show the cycle path for clear diagnostics.
            let cycle_start = self
                .loading_stack
                .iter()
                .position(|m| m == &abs_path_str)
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
                path.span,
            );
            return;
        }

        self.loading_stack.push(abs_path_str.clone());

        // 4. Load and Parse
        let source = match fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                self.loading_stack.retain(|m| m != &abs_path_str);
                self.report_error(
                    format!("Failed to read module '{}': {}", path_str, e),
                    path.span,
                );
                return;
            }
        };

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        let module_ast = match parser.parse() {
            Ok(ast) => ast,
            Err(e) => {
                self.loading_stack.retain(|m| m != &abs_path_str);
                let old_source_override = self.current_source_override.take();
                self.current_source_override =
                    Some((file_path.to_string_lossy().to_string(), source.clone()));
                self.report_syntax_error(&e);
                self.current_source_override = old_source_override;
                return;
            }
        };

        // 5. Check Module Body (merge into current context)
        // `source_dir` is intentionally NOT changed here.  It is set once to
        // the entry-point file's directory and stays fixed for the entire
        // compilation so that every `local.*` import — no matter how deeply
        // nested the importing module is — resolves relative to the project
        // root (the directory that contains the entry-point file).
        let old_module = self.current_module.clone();
        self.current_module = path_str.clone();

        // Snapshot global_scope keys (and their source module) before loading
        // the module so we can (a) restrict visibility for selective imports
        // afterwards and (b) detect namespace collisions.
        let pre_import_globals: HashMap<String, String> = self
            .global_scope
            .iter()
            .map(|(k, v)| (k.clone(), v.module.clone()))
            .collect();
        let pre_import_global_types: HashSet<String> =
            self.global_type_definitions.keys().cloned().collect();

        let old_source_override = self.current_source_override.take();
        self.current_source_override =
            Some((file_path.to_string_lossy().to_string(), source.clone()));

        for stmt in &module_ast.body {
            self.check_statement(stmt, context);
        }

        self.current_source_override = old_source_override;

        // Register module-level alias (e.g., `use system.math as M`).
        // This must happen after the module symbols are loaded so that
        // `infer_member` can look them up in global_scope when resolving `M.foo`.
        if let Some(alias_box) = alias {
            if let ExpressionKind::Identifier(alias_name, _) = &alias_box.node {
                self.module_aliases
                    .insert(alias_name.clone(), path_str.clone());
            }
        }

        // 6. Restrict visibility: only symbols **defined** in the directly imported
        //    module should become visible to the importer.  Transitive dependencies
        //    (types/functions that the imported module itself imported) stay in the
        //    internal stores (`global_type_definitions`, `global_scope`) — needed for
        //    method resolution, vtable generation, etc. — but must not leak into
        //    user-visible namespaces.
        //
        //    For selective imports (`use m.{A, B}`), an additional filter narrows
        //    visibility to only the explicitly listed names.

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

        let module_name = &path_str;

        // --- Detect namespace collisions ---
        if let Some(ref selected) = selected_names {
            for sel_name in selected.keys() {
                if let Some(old_module) = pre_import_globals.get(sel_name) {
                    if let Some(info) = self.global_scope.get(sel_name) {
                        if info.module == *module_name {
                            self.report_error(
                                format!(
                                    "Name '{}' conflicts with an existing definition from \
                                     module '{}'. Use selective imports with an alias to \
                                     disambiguate, e.g. `use {}.{{... as ...}}`.",
                                    sel_name, old_module, module_name
                                ),
                                path.span,
                            );
                        }
                    }
                }
            }
        } else {
            let mut collisions: Vec<(String, String)> = Vec::new();
            for (name, info) in &self.global_scope {
                if info.module == *module_name {
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
                    path.span,
                );
            }
        }

        // --- Helper: should a newly-added name be visible to the importer? ---
        let should_be_visible = |name: &str, def_module: Option<&str>| -> bool {
            // Transitive type (defined in a different module) → never visible.
            let is_from_this_module = def_module.is_none_or(|m| m == module_name.as_str());
            if !is_from_this_module {
                return false;
            }
            // For selective imports, additionally require the name to be listed.
            if let Some(ref selected) = selected_names {
                return selected.contains_key(name);
            }
            true
        };

        // --- Filter global_scope (function/variable symbols) ---
        self.global_scope.retain(|name, info| {
            if !pre_import_globals.contains_key(name) {
                return should_be_visible(name, Some(info.module.as_str()));
            }
            true
        });

        // Also clean up the context's symbol scopes
        if let Some(scope) = context.scopes.last_mut() {
            scope.retain(|name, info| {
                if !pre_import_globals.contains_key(name) {
                    return should_be_visible(name, Some(info.module.as_str()));
                }
                true
            });
        }

        // --- Filter type definitions ---
        // Transitive types are kept in `global_type_definitions` (needed for method
        // resolution and vtable generation) but hidden from user code.  Non-selected
        // types from the direct module are removed entirely.
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
                    return true; // keep in global AND visible
                }
                // Transitive? Keep in global for internal use but hide.
                let is_transitive = def_module.is_some_and(|m| m != module_name.as_str());
                if is_transitive {
                    self.visible_type_names.remove(name);
                    return true;
                }
                // Not transitive, not visible → remove entirely.
                self.visible_type_names.remove(name);
                return false;
            }
            true
        });

        // --- Register item aliases for selective imports ---
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

        // --- Validate that every explicitly selected name was actually exported ---
        //
        // After all retain and alias operations, each name in `selected_names` must
        // resolve to either a symbol in `global_scope` (functions, variables, enum
        // meta-symbols, class constructors, …) or a visible type definition.  If
        // neither is true the name was never defined in the module and we report an
        // error immediately rather than silently accepting the import.
        if let Some(ref selected) = selected_names {
            for (sel_name, sel_span) in selected {
                let in_scope = self
                    .global_scope
                    .get(sel_name.as_str())
                    .is_some_and(|info| info.module == *module_name);

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
                        def_module == Some(module_name.as_str())
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

        // Collect imported statements for MIR lowering and codegen
        self.imported_statements.extend(module_ast.body);

        self.current_module = old_module;

        // Mark as fully loaded and remove from the in-progress stack.
        self.loading_stack.retain(|m| m != &abs_path_str);
        self.loaded_modules.insert(abs_path_str);
    }

    /// Restores visibility for types defined in an already-loaded module.
    ///
    /// When a module M is first loaded by module A, M's types become visible.
    /// A's post-import filter may then hide them (they're transitive to A).
    /// If module B later imports M directly, this method makes M's types
    /// visible again without re-parsing or re-type-checking M.
    fn restore_visibility_for_module(&mut self, module_path: &str, import_kind: &ImportPathKind) {
        let selected_names: Option<HashSet<String>> =
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
            };

        for (name, def) in &self.global_type_definitions {
            let def_module = match def {
                TypeDefinition::Class(cd) => Some(cd.module.as_str()),
                TypeDefinition::Trait(td) => Some(td.module.as_str()),
                TypeDefinition::Struct(sd) => Some(sd.module.as_str()),
                TypeDefinition::Enum(ed) => Some(ed.module.as_str()),
                _ => None,
            };
            if def_module == Some(module_path) {
                if let Some(ref selected) = selected_names {
                    if selected.contains(name.as_str()) {
                        self.visible_type_names.insert(name.clone());
                    }
                } else {
                    self.visible_type_names.insert(name.clone());
                }
            }
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
                            if !parts.is_empty() {
                                let last =
                                    parts.last().unwrap().trim_end_matches(".mi").to_string();
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
