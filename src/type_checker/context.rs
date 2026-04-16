// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type checking context for managing scopes and type information.
//!
//! This module provides the [`Context`] struct which maintains the type checking
//! state during AST traversal, including:
//!
//! - **Symbol scopes**: Stack of variable/function bindings with visibility
//! - **Type definitions**: Struct, enum, class, trait, and alias definitions
//! - **Type hierarchy**: Inheritance and interface relationships
//! - **Linear type tracking**: Consumption state for move semantics
//!
//! # Scope Management
//!
//! The context uses a stack-based scope system where:
//! - `enter_scope()` pushes a new scope for blocks, functions, etc.
//! - `exit_scope()` pops the current scope and checks for unused linear types
//! - Variables are looked up from innermost to outermost scope
//!
//! # Type Definitions
//!
//! Supported type definition kinds:
//! - [`StructDefinition`]: Named product types with fields
//! - [`EnumDefinition`]: Sum types with variants
//! - [`ClassDefinition`]: OOP classes with methods and inheritance
//! - [`TraitDefinition`]: Interface contracts
//! - [`AliasDefinition`]: Type aliases with optional generics
//! - [`GenericDefinition`]: Generic type parameters with constraints

use crate::ast::{literal::Literal, types::*, MemberVisibility};
use crate::error::syntax::Span;
use std::collections::{BTreeMap, HashMap};

/// Represents information about a symbol (variable, function, etc.) in the scope.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub ty: Type,
    pub mutable: bool,
    pub is_constant: bool,
    pub visibility: MemberVisibility,
    pub module: String,
    /// Tracks if a linear resource has been consumed/moved.
    pub consumed: bool,
    /// Optional known compile-time literal value for constants or simple variables
    pub value: Option<Literal>,
    /// When this symbol is an import alias (e.g. `use m.{add as plus}`), the
    /// original symbol name (`"add"`) so MIR lowering can emit the right call target.
    pub original_name: Option<String>,
}

impl SymbolInfo {
    pub fn new(
        ty: Type,
        mutable: bool,
        is_constant: bool,
        visibility: MemberVisibility,
        module: String,
        value: Option<Literal>,
    ) -> Self {
        Self {
            ty,
            mutable,
            is_constant,
            visibility,
            module,
            consumed: false,
            value,
            original_name: None,
        }
    }
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
    // Use BTreeMap for deterministic variant order (crucial for discriminants)
    pub variants: BTreeMap<String, Vec<Type>>,
    pub generics: Option<Vec<GenericDefinition>>,
    /// The module that defines this enum (used for transitive-import filtering).
    pub module: String,
}

/// Definition of a generic type parameter.
#[derive(Debug, Clone)]
pub struct GenericDefinition {
    pub name: String,
    pub constraint: Option<Type>,
    pub kind: TypeDeclarationKind,
}

/// Definition of a type alias (possibly generic).
#[derive(Debug, Clone)]
pub struct AliasDefinition {
    /// The template type with generic placeholders (e.g., T? for Optional<T>)
    pub template: Type,
    /// Generic parameters for this alias (e.g., [T] for Optional<T>)
    pub generics: Option<Vec<GenericDefinition>>,
}

/// Information about a class field.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub ty: Type,
    pub mutable: bool,
    pub visibility: MemberVisibility,
}

/// Information about a method.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub visibility: MemberVisibility,
    pub is_constructor: bool,
    /// Whether this method is abstract (no body).
    pub is_abstract: bool,
}

/// Definition of a class type.
#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub name: String,
    pub generics: Option<Vec<GenericDefinition>>,
    pub base_class: Option<String>,
    pub traits: Vec<String>,
    pub fields: Vec<(String, FieldInfo)>, // Preserves declaration order for constructor and layout
    pub methods: BTreeMap<String, MethodInfo>, // Deterministic method order
    pub module: String,
    /// Whether this class is abstract.
    pub is_abstract: bool,
}

/// Definition of a trait type.
#[derive(Debug, Clone)]
pub struct TraitDefinition {
    pub name: String,
    pub generics: Option<Vec<GenericDefinition>>,
    pub parent_traits: Vec<String>,
    pub methods: BTreeMap<String, MethodInfo>, // Deterministic method order
    pub module: String,
}

/// Enum wrapper for different type definitions.
#[derive(Debug, Clone)]
pub enum TypeDefinition {
    Struct(StructDefinition),
    Enum(EnumDefinition),
    Generic(GenericDefinition),
    Alias(AliasDefinition),
    Class(ClassDefinition),
    Trait(TraitDefinition),
}

/// Collect all fields for a class by walking the inheritance chain from root to leaf.
///
/// Returns fields in declaration order: ancestor fields come before descendant fields.
/// This is the canonical field layout for class instances in codegen and MIR lowering.
pub fn collect_class_fields_all<'a>(
    class_def: &'a ClassDefinition,
    type_definitions: &'a HashMap<String, TypeDefinition>,
) -> Vec<(&'a str, &'a FieldInfo)> {
    let mut chain: Vec<&ClassDefinition> = vec![class_def];
    let mut current = class_def;
    while let Some(base_name) = &current.base_class {
        match type_definitions.get(base_name) {
            Some(TypeDefinition::Class(base)) => {
                chain.push(base);
                current = base;
            }
            _ => break,
        }
    }
    chain.reverse(); // root class first
    chain
        .into_iter()
        .flat_map(|def| def.fields.iter().map(|(n, f)| (n.as_str(), f)))
        .collect()
}

/// Returns `true` if `class_name` or any ancestor in the inheritance chain is abstract,
/// or if the class (or any ancestor) implements at least one trait.
///
/// Both abstract classes and trait-implementing classes use vtable-based virtual dispatch
/// and store a vtable pointer as the first word (offset 0) of their heap payload.
pub fn class_needs_vtable(class_name: &str, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    let mut current: &str = class_name;
    loop {
        match type_defs.get(current) {
            Some(TypeDefinition::Class(cd)) => {
                if cd.is_abstract || !cd.traits.is_empty() {
                    return true;
                }
                match &cd.base_class {
                    Some(base) => current = base,
                    None => return false,
                }
            }
            _ => return false,
        }
    }
}

/// Returns the vtable slot index for `method_name` in the vtable of a class
/// that inherits from `abstract_class_or_trait`.
///
/// When `abstract_class_or_trait` is a trait name, collects all non-constructor
/// methods from the trait hierarchy (sorted alphabetically) and returns the position.
///
/// When it is an abstract class name, collects all non-constructor methods from
/// the full abstract ancestor chain, deduplicated, sorted alphabetically.
pub fn vtable_slot_index(
    abstract_class_or_trait: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<usize> {
    // Handle trait-typed receivers.
    if matches!(
        type_defs.get(abstract_class_or_trait),
        Some(TypeDefinition::Trait(_))
    ) {
        let mut methods = collect_trait_vtable_methods(type_defs, abstract_class_or_trait);
        methods.sort();
        return methods.iter().position(|n| *n == method_name);
    }

    // Collect all abstract ancestors starting from abstract_class (inclusive),
    // walking up the chain.
    let mut abstract_chain: Vec<&str> = Vec::new();
    let mut current: &str = abstract_class_or_trait;
    loop {
        match type_defs.get(current) {
            Some(TypeDefinition::Class(cd)) if cd.is_abstract => {
                abstract_chain.push(current);
                match &cd.base_class {
                    Some(base) => current = base,
                    None => break,
                }
            }
            _ => break,
        }
    }

    // Collect methods from all abstract ancestors (topmost last in chain,
    // so we reverse to process topmost first), deduplicated, alphabetically sorted.
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut all_methods: Vec<&str> = Vec::new();
    for ancestor in abstract_chain.iter().rev() {
        if let Some(TypeDefinition::Class(cd)) = type_defs.get(*ancestor) {
            for (name, m) in &cd.methods {
                if !m.is_constructor && !seen.contains(name.as_str()) {
                    seen.insert(name.as_str());
                    all_methods.push(name.as_str());
                }
            }
        }
    }
    all_methods.sort();

    all_methods.iter().position(|n| *n == method_name)
}

/// Collect all non-constructor method names from a trait and its parent traits.
pub fn collect_trait_vtable_methods<'a>(
    type_defs: &'a HashMap<String, TypeDefinition>,
    trait_name: &str,
) -> Vec<&'a str> {
    let mut methods = Vec::new();
    let mut to_check = vec![trait_name];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(t_name) {
            for (m_name, m_info) in &td.methods {
                if !m_info.is_constructor {
                    methods.push(m_name.as_str());
                }
            }
            to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
        }
    }
    methods
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
    /// Whether we are currently inside a GPU function.
    pub in_gpu_function: bool,
    /// Whether we are currently inside any function.
    pub in_function: bool,
    /// Whether we are currently inside an async function.
    pub in_async_function: bool,
    /// Name of the current class being checked (for self resolution).
    pub current_class: Option<String>,
    /// Name of the base class of the current class (for super resolution).
    pub current_base_class: Option<String>,
    /// The type of the current class (for self expression type inference).
    pub current_class_type: Option<Type>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            type_definitions: vec![HashMap::new()],
            return_types: Vec::new(),
            inferred_return_types: Vec::new(),
            loop_depth: 0,
            in_gpu_function: false,
            in_function: false,
            in_async_function: false,
            current_class: None,
            current_base_class: None,
            current_class_type: None,
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

    /// Defines a symbol (variable, function, etc.) in the current (innermost) scope.
    pub fn define(&mut self, name: String, info: SymbolInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    /// Defines a type in the current scope.
    pub fn define_type(&mut self, name: String, def: TypeDefinition) {
        if let Some(scope) = self.type_definitions.last_mut() {
            scope.insert(name, def);
        }
    }

    /// Updates the type of a defined symbol in the current (innermost) scope.
    pub fn update_symbol_type(&mut self, name: &str, new_type: Type) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.ty = new_type;
                return;
            }
        }
    }

    /// Resolves a symbol by name, searching from the innermost scope outwards.
    /// Returns a reference to avoid cloning; callers should clone only when mutation is needed.
    pub fn resolve_info(&self, name: &str) -> Option<&SymbolInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
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

    /// Checks if a variable is a constant.
    pub fn is_constant(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return info.is_constant;
            }
        }
        false
    }

    /// Marks a symbol as consumed. Returns true if it was already consumed.
    pub fn mark_consumed(&mut self, name: &str) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                if info.consumed {
                    return true;
                }
                info.consumed = true;
                return false;
            }
        }
        false
    }

    /// Checks if a symbol has been consumed.
    pub fn is_consumed(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return info.consumed;
            }
        }
        false
    }

    /// Returns a list of linear variables in the current scope that have not been consumed.
    ///
    /// Each entry contains the variable name and the span from its type declaration,
    /// used for error reporting at scope exit.
    pub fn get_unconsumed_linear_vars(&self) -> Vec<(String, Span)> {
        let mut unconsumed = Vec::new();
        if let Some(scope) = self.scopes.last() {
            for (name, info) in scope {
                if let TypeKind::Linear(_) = &info.ty.kind {
                    if !info.consumed {
                        unconsumed.push((name.clone(), info.ty.span));
                    }
                }
            }
        }
        unconsumed
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

    /// Enters a class context for self/super resolution.
    pub fn enter_class(
        &mut self,
        class_name: String,
        base_class: Option<String>,
        class_type: Type,
    ) {
        self.current_class = Some(class_name);
        self.current_base_class = base_class;
        self.current_class_type = Some(class_type);
    }

    /// Exits the current class context.
    pub fn exit_class(&mut self) {
        self.current_class = None;
        self.current_base_class = None;
        self.current_class_type = None;
    }

    /// Returns true if we are currently inside a class context.
    pub fn in_class(&self) -> bool {
        self.current_class.is_some()
    }
    /// Snapshots the consumed state of all linear variables in all scopes.
    /// Returns a list of scopes, each containing a list of (variable name, consumed state).
    pub fn snapshot_linear_state(&self) -> Vec<Vec<(String, bool)>> {
        self.scopes
            .iter()
            .map(|scope| {
                scope
                    .iter()
                    .filter(|(_, info)| matches!(info.ty.kind, TypeKind::Linear(_)))
                    .map(|(k, v)| (k.clone(), v.consumed))
                    .collect()
            })
            .collect()
    }

    /// Restores the consumed state of linear variables from a snapshot.
    pub fn restore_linear_state(&mut self, snapshot: Vec<Vec<(String, bool)>>) {
        for (i, scope_snapshot) in snapshot.into_iter().enumerate() {
            if i < self.scopes.len() {
                let scope = &mut self.scopes[i];
                for (name, consumed) in scope_snapshot {
                    if let Some(info) = scope.get_mut(&name) {
                        info.consumed = consumed;
                    }
                }
            }
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
