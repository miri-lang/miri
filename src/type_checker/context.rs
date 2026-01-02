// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::{types::*, MemberVisibility};
use crate::error::syntax::Span;
use std::collections::HashMap;

/// Represents information about a symbol (variable, function, etc.) in the scope.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub ty: Type,
    pub mutable: bool,
    pub visibility: MemberVisibility,
    pub module: String,
}

/// Represents relationships between types (inheritance, interfaces, mixins).
#[derive(Debug, Clone, Default)]
pub struct TypeRelation {
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub includes: Vec<String>,
}

/// Definition of a struct type.
#[derive(Debug, Clone)]
pub struct StructDefinition {
    pub fields: Vec<(String, Type, MemberVisibility)>,
    pub generics: Option<Vec<GenericDefinition>>,
    pub module: String,
}

/// Definition of an enum type.
#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub variants: HashMap<String, Vec<Type>>,
    #[allow(dead_code)]
    pub module: String,
}

/// Definition of a generic type parameter.
#[derive(Debug, Clone)]
pub struct GenericDefinition {
    #[allow(dead_code)]
    pub name: String,
    pub constraint: Option<Type>,
    pub kind: TypeDeclarationKind,
}

/// Enum wrapper for different type definitions.
#[derive(Debug, Clone)]
pub enum TypeDefinition {
    Struct(StructDefinition),
    Enum(EnumDefinition),
    Generic(GenericDefinition),
    Alias(Type),
}

/// Context holds the current state of the type checking process, including
/// variable scopes, return types for functions, and loop depth.
pub struct Context {
    /// Stack of scopes for variables. Each scope maps names to SymbolInfo.
    pub scopes: Vec<HashMap<String, SymbolInfo>>,
    /// Stack of scopes for type definitions (e.g. generics inside a function/struct).
    pub type_definitions: Vec<HashMap<String, TypeDefinition>>,
    /// Stack of expected return types for the current function(s).
    pub return_types: Vec<Type>,
    /// Stack of inferred return types (used for lambdas/functions without explicit return type).
    pub inferred_return_types: Vec<Option<Vec<(Type, Span)>>>,
    /// Current depth of nested loops (used to validate break/continue).
    pub loop_depth: usize,
}

impl Context {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            type_definitions: vec![HashMap::new()],
            return_types: Vec::new(),
            inferred_return_types: Vec::new(),
            loop_depth: 0,
        }
    }

    /// Enters a new scope (e.g., block, function).
    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.type_definitions.push(HashMap::new());
    }

    /// Exits the current scope.
    pub fn exit_scope(&mut self) {
        self.scopes.pop();
        self.type_definitions.pop();
    }

    /// Increments loop depth when entering a loop.
    pub fn enter_loop(&mut self) {
        self.loop_depth += 1;
    }

    /// Decrements loop depth when exiting a loop.
    pub fn exit_loop(&mut self) {
        if self.loop_depth > 0 {
            self.loop_depth -= 1;
        }
    }

    /// Defines a variable in the current scope.
    pub fn define(
        &mut self,
        name: String,
        ty: Type,
        mutable: bool,
        visibility: MemberVisibility,
        module: String,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                SymbolInfo {
                    ty,
                    mutable,
                    visibility,
                    module,
                },
            );
        }
    }

    /// Defines a type in the current scope.
    pub fn define_type(&mut self, name: String, def: TypeDefinition) {
        if let Some(scope) = self.type_definitions.last_mut() {
            scope.insert(name, def);
        }
    }

    /// Resolves a symbol by name, searching from the innermost scope outwards.
    pub fn resolve_info(&self, name: &str) -> Option<SymbolInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    /// Checks if a variable is mutable.
    pub fn is_mutable(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return info.mutable;
            }
        }
        false
    }

    /// Resolves a type definition, searching from the innermost scope outwards.
    pub fn resolve_type_definition(&self, name: &str) -> Option<&TypeDefinition> {
        for scope in self.type_definitions.iter().rev() {
            if let Some(def) = scope.get(name) {
                return Some(def);
            }
        }
        None
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
