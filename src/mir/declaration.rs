// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR type declarations for structs, enums, classes, traits, and type aliases.
//!
//! These declarations represent the lowered form of user-defined types,
//! ready for code generation. Unlike the AST representations, MIR declarations
//! have resolved types and are suitable for layout computation.

use crate::ast::types::Type;
use crate::ast::MemberVisibility;

/// A field in a struct or class declaration.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    /// The field name
    pub name: String,
    /// The field's resolved type
    pub ty: Type,
    /// Field visibility (public, private, etc.)
    pub visibility: MemberVisibility,
    /// Field index in the struct
    pub index: usize,
    /// Whether the field is mutable
    pub mutable: bool,
}

/// A struct declaration in MIR.
#[derive(Debug, Clone)]
pub struct StructDecl {
    /// The struct name
    pub name: String,
    /// The struct's fields in declaration order
    pub fields: Vec<FieldDecl>,
    /// Generic type parameter names (if any)
    pub generics: Vec<String>,
    /// Source module
    pub module: String,
}

/// An enum variant declaration.
#[derive(Debug, Clone)]
pub struct VariantDecl {
    /// The variant name
    pub name: String,
    /// The variant's associated types (empty for unit variants)
    pub fields: Vec<Type>,
    /// Discriminant value for this variant
    pub discriminant: usize,
}

/// An enum declaration in MIR.
#[derive(Debug, Clone)]
pub struct EnumDecl {
    /// The enum name
    pub name: String,
    /// The enum's variants
    pub variants: Vec<VariantDecl>,
    /// Source module
    pub module: String,
}

/// A type alias declaration.
#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    /// The alias name
    pub name: String,
    /// The target type this alias refers to
    pub target: Type,
}

/// A method declaration in MIR.
#[derive(Debug, Clone)]
pub struct MethodDecl {
    /// The method name
    pub name: String,
    /// Parameter names and types
    pub params: Vec<(String, Type)>,
    /// Return type
    pub return_type: Type,
    /// Method visibility
    pub visibility: MemberVisibility,
    /// Whether this is a constructor (init)
    pub is_constructor: bool,
}

/// A class declaration in MIR.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    /// The class name
    pub name: String,
    /// The class's fields in declaration order
    pub fields: Vec<FieldDecl>,
    /// The class's methods
    pub methods: Vec<MethodDecl>,
    /// Generic type parameter names (if any)
    pub generics: Vec<String>,
    /// Base class name (single inheritance)
    pub base_class: Option<String>,
    /// Implemented trait names
    pub traits: Vec<String>,
    /// Source module
    pub module: String,
}

/// A trait declaration in MIR.
#[derive(Debug, Clone)]
pub struct TraitDecl {
    /// The trait name
    pub name: String,
    /// The trait's method signatures
    pub methods: Vec<MethodDecl>,
    /// Generic type parameter names (if any)
    pub generics: Vec<String>,
    /// Parent trait names (multiple inheritance for traits)
    pub parent_traits: Vec<String>,
    /// Source module
    pub module: String,
}

/// A top-level declaration in MIR.
#[derive(Debug, Clone)]
pub enum Declaration {
    /// A struct type declaration
    Struct(StructDecl),
    /// An enum type declaration
    Enum(EnumDecl),
    /// A type alias declaration
    TypeAlias(TypeAliasDecl),
    /// A class type declaration
    Class(ClassDecl),
    /// A trait type declaration
    Trait(TraitDecl),
}

impl Declaration {
    /// Get the name of this declaration.
    pub fn name(&self) -> &str {
        match self {
            Declaration::Struct(s) => &s.name,
            Declaration::Enum(e) => &e.name,
            Declaration::TypeAlias(t) => &t.name,
            Declaration::Class(c) => &c.name,
            Declaration::Trait(t) => &t.name,
        }
    }
}
