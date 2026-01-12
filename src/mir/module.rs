// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! MIR module and import support.
//!
//! Handles the lowered form of `use` statements for multi-file compilation.

/// Source of an import path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// Standard library (`system.*`)
    System,
    /// Current project (`local.*`)
    Local,
    /// External package (`<package_name>.*`)
    Package(String),
}

/// Kind of import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// Import all public entities from the module
    All,
    /// Import specific entities
    Named(Vec<ImportItem>),
}

/// A single imported item with optional alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItem {
    /// The original name being imported
    pub name: String,
    /// Optional alias (e.g., `net as network`)
    pub alias: Option<String>,
}

/// A lowered import statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// Source of the import (system, local, or package)
    pub source: ImportSource,
    /// Module path segments (e.g., `["io", "file"]` for `system.io.file`)
    pub path: Vec<String>,
    /// What to import from the module
    pub kind: ImportKind,
    /// Alias for the entire module (when importing module itself)
    pub module_alias: Option<String>,
}

impl Import {
    /// Create a new import with the given source and path.
    pub fn new(source: ImportSource, path: Vec<String>, kind: ImportKind) -> Self {
        Self {
            source,
            path,
            kind,
            module_alias: None,
        }
    }

    /// Set the module alias.
    pub fn with_alias(mut self, alias: String) -> Self {
        self.module_alias = Some(alias);
        self
    }

    /// Get the full import path as a string (e.g., "system.io.file").
    pub fn full_path(&self) -> String {
        let prefix = match &self.source {
            ImportSource::System => "system",
            ImportSource::Local => "local",
            ImportSource::Package(name) => name.as_str(),
        };
        if self.path.is_empty() {
            prefix.to_string()
        } else {
            format!("{}.{}", prefix, self.path.join("."))
        }
    }
}
