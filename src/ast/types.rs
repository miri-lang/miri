// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::error::syntax::Span;
use std::fmt;

/// Identifies a built-in collection type canonically.
///
/// This enum is the single source of truth for the names "Array", "List", "Map", "Set".
/// All compiler logic that needs to identify built-in collections should use
/// `TypeKind::as_builtin_collection()` rather than matching on string literals.
///
/// **Scope**: after Phase 1 (interception registry removed) and Phase 2 (String
/// special-case removed), this enum exists solely to key the **constructor dispatch
/// table** in `mir::lowering::constructors::COLLECTION_CTORS`. It is *not* used for
/// method dispatch; method calls on collections go through normal class method
/// resolution like any other type.
///
/// When a `sizeof<T>` built-in is added to the language, each collection's `init()`
/// can be expressed in pure Miri source and moved to stdlib, at which point the
/// corresponding entry in `COLLECTION_CTORS` (and eventually this enum) can be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinCollectionKind {
    Array,
    List,
    Map,
    Set,
}

impl BuiltinCollectionKind {
    /// Returns the `BuiltinCollectionKind` for a type name string, or `None` if it
    /// is not a built-in collection.  This is the **only** place in the compiler
    /// where the canonical names "Array", "List", "Map", "Set" are written.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Array" => Some(Self::Array),
            "List" => Some(Self::List),
            "Map" => Some(Self::Map),
            "Set" => Some(Self::Set),
            _ => None,
        }
    }

    /// Returns the canonical class name for this built-in collection kind.
    ///
    /// This is the reverse of [`from_name`]: the returned string is the single
    /// authoritative spelling used everywhere the compiler needs the class name
    /// as a string (e.g. for method-dispatch mangling or registry look-ups).
    pub fn name(self) -> &'static str {
        match self {
            Self::Array => "Array",
            Self::List => "List",
            Self::Map => "Map",
            Self::Set => "Set",
        }
    }
}

/// Data for a function type, boxed to reduce `TypeKind` enum size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionTypeData {
    pub generics: Option<Vec<Expression>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<Box<Expression>>,
}

/// Represents a type expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

impl Type {
    pub fn new(kind: TypeKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Returns true if this type has Copy semantics (can be duplicated without invalidating source).
    /// Primitive types (integers, floats, booleans) are Copy.
    /// Complex types (strings, lists, maps, custom types) require Move.
    pub fn is_copy(&self) -> bool {
        self.kind.is_copy()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    /// System-dependent arbitrary precision integer (int32 or int64).
    Int,
    /// 8-bit signed integer.
    I8,
    /// 16-bit signed integer.
    I16,
    /// 32-bit signed integer.
    I32,
    /// 64-bit signed integer.
    I64,
    /// 128-bit signed integer.
    I128,
    /// 8-bit unsigned integer.
    U8,
    /// 16-bit unsigned integer.
    U16,
    /// 32-bit unsigned integer.
    U32,
    /// 64-bit unsigned integer.
    U64,
    /// 128-bit unsigned integer.
    U128,
    /// Arbitrary precision float.
    Float,
    /// 32-bit floating point.
    F32,
    /// 64-bit floating point.
    F64,
    /// String type.
    String,
    /// Boolean type.
    Boolean,
    /// Identifier type (internal, used for function/type references in MIR).
    Identifier,
    /// Raw pointer type (platform-width, opaque).
    ///
    /// Used in runtime/intrinsic function declarations and private class
    /// fields for type-erased FFI. Maps to the target's pointer width
    /// (e.g., I64 on 64-bit, I32 on 32-bit).
    RawPtr,
    /// List type (e.g., `[i32]`).
    ///
    /// **Parser-only variant.** Normalized to `TypeKind::Custom("List", [T])`
    /// by [`crate::ast::normalize`] before any downstream compiler phase sees it.
    List(Box<Expression>),
    /// Array type (e.g., `[i32; 4]`).
    ///
    /// **Parser-only variant.** Normalized to `TypeKind::Custom("Array", [T, N])`
    /// by [`crate::ast::normalize`] before any downstream compiler phase sees it.
    Array(Box<Expression>, Box<Expression>),
    /// Map type (e.g., `{string: i32}`).
    ///
    /// **Parser-only variant.** Normalized to `TypeKind::Custom("Map", [K, V])`
    /// by [`crate::ast::normalize`] before any downstream compiler phase sees it.
    Map(Box<Expression>, Box<Expression>),
    /// Tuple type (e.g., `(i32, string)`).
    Tuple(Vec<Expression>),
    /// Set type (e.g., `{i32}`).
    ///
    /// **Parser-only variant.** Normalized to `TypeKind::Custom("Set", [T])`
    /// by [`crate::ast::normalize`] before any downstream compiler phase sees it.
    Set(Box<Expression>),
    /// Result type (e.g., `result<i32, string>`).
    Result(Box<Expression>, Box<Expression>),
    /// Future type (e.g., `future<i32>`).
    Future(Box<Expression>),
    /// Function type (e.g., `fn<T>(x int) float`). Boxed to reduce enum size.
    Function(Box<FunctionTypeData>),

    /// Generic type (e.g., `T extends SomeClass`).
    Generic(String, Option<Box<Type>>, TypeDeclarationKind),

    /// Custom type (e.g., struct name).
    Custom(String, Option<Vec<Expression>>),
    /// Metatype (type of a type).
    Meta(Box<Type>),
    /// Option type wrapper (e.g., `T?` or `Option<T>`).
    Option(Box<Type>),
    /// Void type.
    Void,
    /// Error type (for type checking).
    Error,
    /// Linear type wrapper (explicit ownership).
    Linear(Box<Type>),
}

/// Represents a type declaration kind
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDeclarationKind {
    None,
    Is,
    Extends,
    Implements,
    Includes,
}

impl TypeKind {
    /// Returns true if this type kind has Copy semantics.
    /// Primitive types (integers, floats, booleans, void) are Copy.
    /// Complex types (strings, lists, maps, custom types) require Move.
    pub fn is_copy(&self) -> bool {
        match self {
            // Primitives are Copy
            TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Void
            | TypeKind::Error => true,
            // Linear types are never Copy
            TypeKind::Linear(_) => false,
            // Complex types require Move
            TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Custom(_, _)
            | TypeKind::Meta(_) => false,
            // Option: inherits from inner type
            TypeKind::Option(inner) => inner.kind.is_copy(),
            // Tuple: Check that all elements are Copy (simplified - we'd need to resolve types)
            // For now, treat tuples as Copy since lowering doesn't track element types here
            TypeKind::Tuple(_) => true,
        }
    }

    /// Returns the `BuiltinCollectionKind` if this type is a built-in collection,
    /// for either the canonical variant (`TypeKind::List(...)`) or a class reference
    /// (`TypeKind::Custom("List", ...)`).  Returns `None` for all other types.
    pub fn as_builtin_collection(&self) -> Option<BuiltinCollectionKind> {
        match self {
            TypeKind::Array(_, _) => Some(BuiltinCollectionKind::Array),
            TypeKind::List(_) => Some(BuiltinCollectionKind::List),
            TypeKind::Map(_, _) => Some(BuiltinCollectionKind::Map),
            TypeKind::Set(_) => Some(BuiltinCollectionKind::Set),
            TypeKind::Custom(name, _) => BuiltinCollectionKind::from_name(name),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl fmt::Display for TypeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeKind::Int => write!(f, "int"),
            TypeKind::I8 => write!(f, "i8"),
            TypeKind::I16 => write!(f, "i16"),
            TypeKind::I32 => write!(f, "i32"),
            TypeKind::I64 => write!(f, "i64"),
            TypeKind::I128 => write!(f, "i128"),
            TypeKind::U8 => write!(f, "u8"),
            TypeKind::U16 => write!(f, "u16"),
            TypeKind::U32 => write!(f, "u32"),
            TypeKind::U64 => write!(f, "u64"),
            TypeKind::U128 => write!(f, "u128"),
            TypeKind::Float => write!(f, "float"),
            TypeKind::F32 => write!(f, "f32"),
            TypeKind::F64 => write!(f, "f64"),
            TypeKind::String => write!(f, "String"),
            TypeKind::Boolean => write!(f, "bool"),
            TypeKind::Identifier => write!(f, "identifier"),
            TypeKind::RawPtr => write!(f, "RawPtr"),
            TypeKind::List(inner) => write!(f, "List({})", inner.node),
            TypeKind::Array(inner, size) => write!(f, "Array({}, {})", inner.node, size.node),
            TypeKind::Map(k, v) => write!(f, "Map({}, {})", k.node, v.node),
            TypeKind::Tuple(elements) => {
                write!(f, "Tuple(")?;
                if let Some((first, rest)) = elements.split_first() {
                    write!(f, "{}", first.node)?;
                    for e in rest {
                        write!(f, ", {}", e.node)?;
                    }
                }
                write!(f, ")")
            }
            TypeKind::Set(inner) => write!(f, "Set({})", inner.node),
            TypeKind::Result(ok, err) => write!(f, "Result({}, {})", ok.node, err.node),
            TypeKind::Future(inner) => write!(f, "Future({})", inner.node),
            TypeKind::Function(func) => {
                write!(f, "Function(")?;
                if let Some((first, rest)) = func.params.split_first() {
                    write!(f, "{}", first.typ.node)?;
                    for p in rest {
                        write!(f, ", {}", p.typ.node)?;
                    }
                }
                write!(f, ")")?;
                if let Some(ret) = &func.return_type {
                    write!(f, " -> {}", ret.node)?;
                }
                Ok(())
            }
            TypeKind::Generic(name, _, _) => write!(f, "{}", name),
            TypeKind::Custom(name, args) => {
                write!(f, "{}", name)?;
                if let Some(args) = args {
                    // Builtin collections use parenthesis notation (e.g. `Array(int, 3)`)
                    // to match the display style of the old canonical variants.
                    // Generic user-defined types use angle bracket notation (e.g. `Foo<T>`).
                    let (open, close) = if BuiltinCollectionKind::from_name(name.as_str()).is_some()
                    {
                        ('(', ')')
                    } else {
                        ('<', '>')
                    };
                    write!(f, "{}", open)?;
                    if let Some((first, rest)) = args.split_first() {
                        write!(f, "{}", first.node)?;
                        for arg in rest {
                            write!(f, ", {}", arg.node)?;
                        }
                    }
                    write!(f, "{}", close)?;
                }
                Ok(())
            }
            TypeKind::Meta(inner) => write!(f, "meta({})", inner),
            TypeKind::Option(inner) => write!(f, "{}?", inner),
            TypeKind::Void => write!(f, "void"),
            TypeKind::Error => write!(f, "error"),
            TypeKind::Linear(inner) => write!(f, "linear({})", inner),
        }
    }
}

impl fmt::Display for TypeDeclarationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDeclarationKind::None => write!(f, ""),
            TypeDeclarationKind::Is => write!(f, "is"),
            TypeDeclarationKind::Extends => write!(f, "extends"),
            TypeDeclarationKind::Implements => write!(f, "implements"),
            TypeDeclarationKind::Includes => write!(f, "includes"),
        }
    }
}
