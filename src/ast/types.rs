// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::error::syntax::Span;
use std::fmt;

/// Identifies a built-in collection type canonically.
///
/// Used to key the constructor dispatch table in
/// `mir::lowering::constructors::COLLECTION_CTORS`. Prefer
/// `TypeKind::as_builtin_collection()` over matching on raw class-name strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinCollectionKind {
    Array,
    List,
    Map,
    Set,
}

/// Canonical name of the built-in `Cloneable` trait.
///
/// Centralized so codegen and runtime-helper dispatch share one source of
/// truth instead of scattering `"Cloneable"` string literals across the
/// compiler. Mirrors the role of [`BuiltinCollectionKind`] / [`RESULT_TYPE_NAME`].
pub const CLONEABLE_TRAIT_NAME: &str = "Cloneable";

/// Canonical name of the stdlib `Accelerable` capability trait.
///
/// The residency gate dispatches on this trait to decide whether a type may
/// back a `gpu let` / `gpu var` binding — it never matches a GPU-eligible type
/// name. Centralized so the gate and any future marshalling registry share one
/// spelling. Mirrors [`CLONEABLE_TRAIT_NAME`].
pub const ACCELERABLE_TRAIT_NAME: &str = "Accelerable";

/// Canonical class name for the built-in `Result<T, E>` sum type.
///
/// `TypeKind::Result(ok, err)` is normalized to
/// `TypeKind::Custom(RESULT_TYPE_NAME, [ok, err])` by `resolve_type_kind` in
/// the type checker; the factory uses the same name when producing already-
/// normalized Result types. Centralized here to keep the spelling in one place.
pub const RESULT_TYPE_NAME: &str = "Result";

/// Canonical class name for homogeneous tuples (`system.collections.tuple`).
///
/// Heterogeneous tuples use [`TypeKind::Tuple`]; the type checker normalizes
/// homogeneous tuples to `TypeKind::Custom(TUPLE_TYPE_NAME, [elem_ty])` so
/// they pick up the `Tuple<T>` class methods. Use [`TypeKind::is_tuple`] to
/// recognize both forms instead of string-matching on this constant.
pub const TUPLE_TYPE_NAME: &str = "Tuple";

/// Canonical class name for the built-in `Option<T>` sum type.
///
/// `TypeKind::Option(t)` is the canonical variant; the type checker also
/// recognizes `TypeKind::Custom(OPTION_TYPE_NAME, [t])` produced from generic
/// instantiations. Centralized so dispatch and pattern-match code share one
/// spelling instead of scattering `"Option"` literals.
pub const OPTION_TYPE_NAME: &str = "Option";

/// Canonical class name for the built-in `String` type when referred to by
/// class-method dispatch (e.g. `String_length`). The primitive form is
/// [`TypeKind::String`]; this constant is used for symbol mangling and
/// stdlib method-registry lookups.
pub const STRING_TYPE_NAME: &str = "String";

/// Canonical class name for the compiler-builtin `Dim3` struct used in GPU
/// kernel context fields.
pub const DIM3_TYPE_NAME: &str = "Dim3";

/// Canonical class name for the compiler-builtin `GpuContext` struct made
/// available inside `gpu fn` bodies.
pub const GPU_CONTEXT_TYPE_NAME: &str = "GpuContext";

/// Canonical class name for the opaque `Kernel` handle returned by `gpu fn`s.
pub const KERNEL_TYPE_NAME: &str = "Kernel";

/// Canonical class name for the stdlib `GpuArray<T>` type — the only managed
/// container that is GPU-compatible inside `gpu fn` bodies.
pub const GPU_ARRAY_TYPE_NAME: &str = "GpuArray";

impl BuiltinCollectionKind {
    /// Returns the `BuiltinCollectionKind` for a class name, or `None` if the
    /// name does not match a built-in collection.
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

    /// True if `method_name` mutates the underlying collection storage and so
    /// requires a copy-on-write guard before lowering.
    ///
    /// Methods handled by inline intrinsics (`push`, `insert`, `set` on List)
    /// emit their own CoW check and are excluded here.
    pub fn mutates_method(self, method_name: &str) -> bool {
        match self {
            Self::List => matches!(
                method_name,
                "pop" | "remove" | "remove_at" | "clear" | "sort" | "reverse"
            ),
            Self::Set => matches!(method_name, "add" | "remove" | "clear"),
            Self::Map => matches!(method_name, "set" | "remove" | "clear"),
            Self::Array => false,
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
    ///
    /// Displayed as `"String"` (capital) to match the canonical stdlib class
    /// spelling used in type-checker error messages and MIR dispatch.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            // Without resolved element types we cannot prove Copy for a tuple,
            // so default to Move. Perceus then inserts the IncRef/DecRef pair;
            // callers with resolved element types can override per-element.
            TypeKind::Tuple(_) => false,
        }
    }

    /// Returns true for either the canonical heterogeneous tuple
    /// (`TypeKind::Tuple(...)`) or the post-normalization homogeneous form
    /// (`TypeKind::Custom(TUPLE_TYPE_NAME, ...)`). Centralizing this check
    /// keeps the `"Tuple"` spelling out of downstream dispatch logic.
    pub fn is_tuple(&self) -> bool {
        match self {
            TypeKind::Tuple(_) => true,
            TypeKind::Custom(name, _) => name == TUPLE_TYPE_NAME,
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
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Linear(_)
            | TypeKind::Void
            | TypeKind::Error => false,
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
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Tuple(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Linear(_)
            | TypeKind::Void
            | TypeKind::Error => None,
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
            TypeKind::Int => f.write_str("int"),
            TypeKind::I8 => f.write_str("i8"),
            TypeKind::I16 => f.write_str("i16"),
            TypeKind::I32 => f.write_str("i32"),
            TypeKind::I64 => f.write_str("i64"),
            TypeKind::I128 => f.write_str("i128"),
            TypeKind::U8 => f.write_str("u8"),
            TypeKind::U16 => f.write_str("u16"),
            TypeKind::U32 => f.write_str("u32"),
            TypeKind::U64 => f.write_str("u64"),
            TypeKind::U128 => f.write_str("u128"),
            TypeKind::Float => f.write_str("float"),
            TypeKind::F32 => f.write_str("f32"),
            TypeKind::F64 => f.write_str("f64"),
            TypeKind::String => f.write_str("String"),
            TypeKind::Boolean => f.write_str("bool"),
            TypeKind::Identifier => f.write_str("identifier"),
            TypeKind::RawPtr => f.write_str("RawPtr"),
            TypeKind::Void => f.write_str("void"),
            TypeKind::Error => f.write_str("error"),
            TypeKind::List(inner) => write!(f, "List({})", inner.node),
            TypeKind::Array(inner, size) => write!(f, "Array({}, {})", inner.node, size.node),
            TypeKind::Map(k, v) => write!(f, "Map({}, {})", k.node, v.node),
            TypeKind::Set(inner) => write!(f, "Set({})", inner.node),
            TypeKind::Result(ok, err) => write!(f, "Result({}, {})", ok.node, err.node),
            TypeKind::Future(inner) => write!(f, "Future({})", inner.node),
            TypeKind::Meta(inner) => write!(f, "meta({})", inner),
            TypeKind::Option(inner) => write!(f, "{}?", inner),
            TypeKind::Linear(inner) => write!(f, "linear({})", inner),
            TypeKind::Generic(name, _, _) => f.write_str(name),
            TypeKind::Tuple(elements) => fmt_tuple(f, elements),
            TypeKind::Function(func) => fmt_function(f, func),
            TypeKind::Custom(name, args) => fmt_custom(f, name, args.as_deref()),
        }
    }
}

fn fmt_tuple(f: &mut fmt::Formatter<'_>, elements: &[Expression]) -> fmt::Result {
    f.write_str("Tuple(")?;
    if let Some((first, rest)) = elements.split_first() {
        write!(f, "{}", first.node)?;
        for e in rest {
            write!(f, ", {}", e.node)?;
        }
    }
    f.write_str(")")
}

fn fmt_function(f: &mut fmt::Formatter<'_>, func: &FunctionTypeData) -> fmt::Result {
    f.write_str("Function(")?;
    if let Some((first, rest)) = func.params.split_first() {
        write!(f, "{}", first.typ.node)?;
        for p in rest {
            write!(f, ", {}", p.typ.node)?;
        }
    }
    f.write_str(")")?;
    if let Some(ret) = &func.return_type {
        write!(f, " -> {}", ret.node)?;
    }
    Ok(())
}

fn fmt_custom(f: &mut fmt::Formatter<'_>, name: &str, args: Option<&[Expression]>) -> fmt::Result {
    f.write_str(name)?;
    let Some(args) = args else { return Ok(()) };
    let (open, close) = if BuiltinCollectionKind::from_name(name).is_some() {
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
    write!(f, "{}", close)
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
