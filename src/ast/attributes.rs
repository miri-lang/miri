// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::syntax::Span;

/// Marks an enum whose variant set may grow, so matches outside its defining
/// module must carry an `else` arm.
pub const NON_EXHAUSTIVE_ATTRIBUTE: &str = "non_exhaustive";

/// Marks a type whose values may not be silently discarded.
pub const MUST_USE_ATTRIBUTE: &str = "must_use";

/// Marks a declaration as deprecated with an optional migration message.
pub const DEPRECATED_ATTRIBUTE: &str = "deprecated";

/// Represents a declaration that can carry attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeTarget {
    Enum,
    Function,
    Class,
}

impl std::fmt::Display for AttributeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            AttributeTarget::Enum => "an enum declaration",
            AttributeTarget::Function => "a function declaration",
            AttributeTarget::Class => "a class declaration",
        };
        write!(f, "{}", name)
    }
}

/// Represents the spelling of an attribute — either the modern `@name` form or a deprecated keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeSpelling {
    /// Modern attribute syntax: `@name` or `@name("argument")`
    Attribute,
    /// Deprecated keyword form: e.g. `must_use enum Foo`
    DeprecatedKeyword,
}

/// An attribute on a declaration. Attributes are compiler-known markers,
/// not macros. Each attribute has a name and optionally one string-literal argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub argument: Option<String>,
    pub span: Span,
    pub spelling: AttributeSpelling,
}

impl Attribute {
    pub fn new(
        name: String,
        argument: Option<String>,
        span: Span,
        spelling: AttributeSpelling,
    ) -> Self {
        Attribute {
            name,
            argument,
            span,
            spelling,
        }
    }
}

// Hashed by name, argument, and spelling only: `Span` carries no `Hash` impl.
// Equality still compares the span, so equal attributes always hash equally.
impl std::hash::Hash for Attribute {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.argument.hash(state);
        self.spelling.hash(state);
    }
}
