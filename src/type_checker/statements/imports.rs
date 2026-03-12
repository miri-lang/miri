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
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

impl TypeChecker {
    pub(crate) fn check_use(
        &mut self,
        path: &Expression,
        _alias: &Option<Box<Expression>>,
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

        // Security: Prevent path traversal by structurally rejecting
        // directory traversal patterns and absolute path anchors in the input string
        // *before* any path transformations are applied.
        if path_str.contains("..") || path_str.contains('/') || path_str.contains('\\') {
            self.report_error(
                format!(
                    "Invalid import path '{}': path traversal is not allowed",
                    path_str
                ),
                path.span,
            );
            return;
        }

        // 2. Resolve file path
        // Assume src/stdlib for now.
        // Convert "system.io" -> "system/io.mi"
        let relative_path = path_str.replace(".", "/") + ".mi";

        let stdlib_base = PathBuf::from("src/stdlib");
        let current_dir = std::env::current_dir().unwrap_or_default();

        let possible_locations = vec![
            (stdlib_base.clone(), stdlib_base.join(&relative_path)),
            (current_dir.clone(), current_dir.join(&relative_path)),
        ];

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
            return; // Already loaded
        }
        self.loaded_modules.insert(abs_path_str.clone());

        // 4. Load and Parse
        let source = match fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
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
                self.report_error(
                    format!("Failed to parse module '{}': {:?}", path_str, e),
                    path.span,
                );
                return;
            }
        };

        // 5. Check Module Body (merge into current context)
        // Store current module name
        let old_module = self.current_module.clone();
        self.current_module = path_str.clone();

        // Snapshot global_scope keys before loading the module so we can
        // restrict visibility for selective imports afterwards.
        let pre_import_globals: HashSet<String> = self.global_scope.keys().cloned().collect();
        let pre_import_types: HashSet<String> = context
            .type_definitions
            .last()
            .map(|scope| scope.keys().cloned().collect())
            .unwrap_or_default();

        for stmt in &module_ast.body {
            self.check_statement(stmt, context);
        }

        // 6. For selective imports, restrict visibility to only the named items.
        // The entire module was type-checked above (needed for internal consistency),
        // but only the explicitly listed symbols should be visible to user code.
        if let ImportPathKind::Multi(ref items) = import_kind {
            let selected_names: HashSet<String> = items
                .iter()
                .filter_map(|(expr, _alias)| {
                    if let ExpressionKind::Identifier(name, _) = &expr.node {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();

            // Remove symbols added by this module that are not in the selective list
            let module_name = &path_str;
            self.global_scope.retain(|name, info| {
                if info.module == *module_name
                    && !pre_import_globals.contains(name)
                    && !selected_names.contains(name)
                {
                    return false;
                }
                true
            });

            // Remove type definitions added by this module that are not selected
            if let Some(scope) = context.type_definitions.last_mut() {
                scope.retain(|name, _| {
                    if !pre_import_types.contains(name) && !selected_names.contains(name) {
                        return false;
                    }
                    true
                });
            }

            // Also clean up the context's symbol scopes
            if let Some(scope) = context.scopes.last_mut() {
                scope.retain(|name, info| {
                    if info.module == *module_name
                        && !pre_import_globals.contains(name)
                        && !selected_names.contains(name)
                    {
                        return false;
                    }
                    true
                });
            }
        }

        // Collect imported statements for MIR lowering and codegen
        self.imported_statements.extend(module_ast.body);

        self.current_module = old_module;
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
}
