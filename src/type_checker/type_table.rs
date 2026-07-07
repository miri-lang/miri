// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Internal type-state management for type checking.
//!
//! This module provides the [`TypeTable`] struct, which encapsulates the
//! type definitions, symbol information, type hierarchy relationships, and
//! visibility tracking that were previously directly on [`TypeChecker`].
//!
//! [`TypeChecker`]: super::TypeChecker

use super::context::{SymbolInfo, TypeDefinition, TypeRelation};
use crate::ast::types::Type;
use std::collections::{HashMap, HashSet};

/// Internal type-state management for the type checker.
///
/// This struct encapsulates all type definition, symbol, hierarchy, and
/// visibility fields that were previously directly on [`TypeChecker`],
/// providing better separation of concerns.
///
/// [`TypeChecker`]: super::TypeChecker
#[derive(Debug)]
pub(crate) struct TypeTable {
    /// Maps expression IDs to their inferred types.
    pub(crate) types: HashMap<usize, Type>,
    /// Stores type hierarchy relationships (extends, implements, includes)
    pub(crate) hierarchy: HashMap<String, TypeRelation>,
    /// Global scope: function/variable declarations visible across modules.
    pub(crate) global_scope: HashMap<String, SymbolInfo>,
    /// Type definitions (class, struct, trait, enum) indexed by name.
    pub(crate) global_type_definitions: HashMap<String, TypeDefinition>,
    /// Tracks which type names are visible to user code. Types in
    /// `global_type_definitions` but NOT in this set are internal-only
    /// (e.g. transitive trait dependencies kept for vtable generation).
    pub(crate) visible_type_names: HashSet<String>,
}

impl TypeTable {
    /// Creates a new type table with the given definitions and visibility.
    pub(crate) fn new(
        global_scope: HashMap<String, SymbolInfo>,
        global_type_definitions: HashMap<String, TypeDefinition>,
    ) -> Self {
        let visible_type_names = global_type_definitions.keys().cloned().collect();
        Self {
            types: HashMap::new(),
            hierarchy: HashMap::new(),
            global_scope,
            global_type_definitions,
            visible_type_names,
        }
    }
}
