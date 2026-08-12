// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Module loading and visibility tracking for type checking.
//!
//! This module provides the [`ModuleLoader`] struct, which manages the module
//! loading state, circular import detection, module visibility tracking, and
//! module-level metadata for the type checker.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Tracks module loading state, visibility, and metadata.
///
/// This struct encapsulates all module-related fields and operations that were
/// previously directly on [`TypeChecker`], providing better separation of concerns.
///
/// [`TypeChecker`]: super::TypeChecker
#[derive(Debug, Clone)]
pub struct ModuleLoader {
    /// Name of the current module/class being checked.
    pub current_module: String,
    /// Set of modules (by absolute path) that have been fully loaded.
    pub loaded_modules: HashSet<String>,
    /// For each fully-loaded module (keyed by module path), the type names its
    /// load made user-visible — including transitive ones it pulled in. A repeat
    /// `use` of an already-loaded module replays this set so a guarded re-import
    /// exposes the same names a fresh load would have.
    pub module_visibility: HashMap<String, Vec<String>>,
    /// Module paths the implicit prelude preloaded for definitions only (the
    /// collection-literal backings). Because the preload marks them loaded, a
    /// user's later explicit `use` of one is a guarded re-import; replaying its
    /// full visibility (transitive types included) is restricted to this set so
    /// ordinary user re-imports keep their own-types-only visibility.
    pub implicitly_preloaded_modules: HashSet<String>,
    /// Type names a program declared itself instead of taking them from the
    /// shadowable prelude tier, whose module was therefore never loaded.
    /// Importing a module that needs one of these names is a conflict: it was
    /// written against the stdlib type, not the program's.
    pub user_shadowed_types: HashSet<String>,
    /// Stack of modules currently being loaded (used to detect circular imports).
    pub loading_stack: Vec<String>,
    /// Maps module alias names to their full module paths.
    /// e.g., `"M"` → `"system.math"` for `use system.math as M`.
    pub module_aliases: HashMap<String, String>,
    /// Directory of the source file being compiled, used to resolve `local.*` imports.
    pub source_dir: Option<PathBuf>,
    /// When set, errors are tagged with this (file_path, source_text) so that
    /// the formatter can display the correct source context for imported files.
    pub current_source_override: Option<(String, String)>,
    /// Names of classes/traits inserted by the cross-module pre-pass as
    /// partial placeholders so forward references resolve during recursive
    /// module loading. `check_class` / `check_trait` recognize members of
    /// this set as overwritable and remove the name on full registration.
    pub pre_registered_types: HashSet<String>,
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleLoader {
    /// Creates a new module loader with default state.
    pub fn new() -> Self {
        Self {
            current_module: "Main".to_string(),
            loaded_modules: HashSet::new(),
            module_visibility: HashMap::new(),
            implicitly_preloaded_modules: HashSet::new(),
            user_shadowed_types: HashSet::new(),
            loading_stack: Vec::new(),
            module_aliases: HashMap::new(),
            source_dir: None,
            current_source_override: None,
            pre_registered_types: HashSet::new(),
        }
    }

    /// Names the module at `module_path` exposes that the program declared
    /// itself, sorted for a stable diagnostic order. Empty unless the module has
    /// been loaded, which is when its exposed names are known.
    pub fn shadowed_names_exposed_by(&self, module_path: &str) -> Vec<String> {
        let Some(names) = self.module_visibility.get(module_path) else {
            return Vec::new();
        };
        let mut conflicts: Vec<String> = names
            .iter()
            .filter(|name| self.user_shadowed_types.contains(*name))
            .cloned()
            .collect();
        conflicts.sort();
        conflicts
    }
}
