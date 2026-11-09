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

/// Marks a function as a test to be run by the test runner.
pub const TEST_ATTRIBUTE: &str = "test";

/// Marks a test function as ignored, with a reason for why.
pub const IGNORE_ATTRIBUTE: &str = "ignore";

/// Marks a test function as expected to fail, with a reason why.
pub const XFAIL_ATTRIBUTE: &str = "xfail";

/// Marks a method parameter as the index into a collection, enabling compile-time
/// bounds checking. The argument is the parameter name (e.g., `@index_bounds_check("index")`).
pub const INDEX_BOUNDS_CHECK_ATTRIBUTE: &str = "index_bounds_check";

/// Marks two method parameters as the `[start, end)` range into the receiver,
/// enabling compile-time range checking when the receiver's size is known. The
/// argument names both parameters (e.g., `@index_range_check("start, end")`).
pub const INDEX_RANGE_CHECK_ATTRIBUTE: &str = "index_range_check";

/// Marks the static method a regex literal (`re"pattern"`) is built by. It
/// takes the pattern, already validated by the compiler, as its one `String`
/// argument, and its class is the type of every regex literal.
pub const PATTERN_LITERAL_ATTRIBUTE: &str = "regex_literal";

/// Marks the method of the regex literal's class a regex match pattern tests
/// its subject with: it takes the subject `String` and answers `bool`.
pub const PATTERN_MATCH_ATTRIBUTE: &str = "regex_match";
/// Names the parameters of a `runtime` declaration the entry point only reads:
/// it keeps no reference to them and writes nothing reached through them.
pub const READS_ATTRIBUTE: &str = "reads";

/// Names the parameters of a `runtime` declaration the entry point writes in
/// place, keeping no reference to them past the call.
pub const WRITES_ATTRIBUTE: &str = "writes";

/// Names the element parameters of a `runtime` declaration the collection
/// keeps: the entry point takes the caller's reference over.
pub const STORES_ATTRIBUTE: &str = "stores";

/// Names the parameters of a `runtime` declaration that carry an element of a
/// list, an array or a set. Such an argument travels by address.
pub const ELEMENT_ATTRIBUTE: &str = "element";

/// Names the parameters of a `runtime` declaration that carry a key of a map.
pub const KEY_ATTRIBUTE: &str = "key";

/// Names the parameters of a `runtime` declaration that carry a value of a map.
pub const VALUE_ATTRIBUTE: &str = "value";

/// States the closure a `runtime` declaration's entry point calls: the closure
/// parameter, how many arguments the runtime passes it, and the collection
/// parameter whose elements those are, or `none`
/// (e.g. `@closure("f, 2, l")`).
pub const CLOSURE_ATTRIBUTE: &str = "closure";

/// Names the container parameter whose elements a `runtime` declaration's
/// entry point orders.
pub const ORDERS_ATTRIBUTE: &str = "orders";

/// Names the container parameter whose elements, or keys, a `runtime`
/// declaration's entry point matches values against.
pub const MATCHES_ATTRIBUTE: &str = "matches";

/// Marks a `runtime` declaration whose entry point hands back an element,
/// written into storage the caller names.
pub const RETURNS_ELEMENT_ATTRIBUTE: &str = "returns_element";

/// Names the collection parameter a `runtime` declaration's entry point hands
/// back a fresh copy of.
pub const COPIES_ATTRIBUTE: &str = "copies";

/// Marks a `runtime` declaration whose entry point builds an empty collection.
pub const ALLOCATES_ATTRIBUTE: &str = "allocates";

/// Marks a function that never returns to its caller, or a `runtime`
/// declaration whose entry point does not.
pub const DIVERGES_ATTRIBUTE: &str = "diverges";

/// Represents a declaration that can carry attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeTarget {
    Enum,
    Function,
    Class,
    RuntimeFunction,
}

impl std::fmt::Display for AttributeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            AttributeTarget::Enum => "an enum declaration",
            AttributeTarget::Function => "a function declaration",
            AttributeTarget::Class => "a class declaration",
            AttributeTarget::RuntimeFunction => "a runtime function declaration",
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
#[derive(Debug, Clone, Eq)]
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

/// Equality ignores `span`, matching [`crate::ast::node::IdNode`] and
/// `FunctionDeclarationData`: a node's source location is metadata rather than
/// part of its identity. Without this, the same attribute written at two
/// offsets compares unequal, which would make an attributed declaration
/// unrecognisable across a re-render. Nothing in the compiler compares whole
/// attributes — validation matches on `name` — so this only narrows identity to
/// what actually identifies an attribute.
impl PartialEq for Attribute {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.argument == other.argument
            && self.spelling == other.spelling
    }
}

// Hashed by name, argument, and spelling — the same fields equality compares.
impl std::hash::Hash for Attribute {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.argument.hash(state);
        self.spelling.hash(state);
    }
}
