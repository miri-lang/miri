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

/// Canonical class name for the compiler-builtin `Vec2<T>` generic vector type.
pub const VEC2_TYPE_NAME: &str = "Vec2";

/// Canonical class name for the compiler-builtin `Vec3<T>` generic vector type.
pub const VEC3_TYPE_NAME: &str = "Vec3";

/// Canonical class name for the compiler-builtin `Vec4<T>` generic vector type.
pub const VEC4_TYPE_NAME: &str = "Vec4";

/// Canonical class name for the compiler-builtin `GpuContext` struct made
/// available inside `gpu fn` bodies.
pub const GPU_CONTEXT_TYPE_NAME: &str = "GpuContext";

/// Canonical class name for the opaque `Kernel` handle returned by `gpu fn`s.
pub const KERNEL_TYPE_NAME: &str = "Kernel";

/// Canonical implicit identifier bound to the kernel context inside `gpu fn`
/// bodies. Exposes `thread_idx` / `block_idx` / `block_dim` / `grid_dim`.
pub const KERNEL_CONTEXT_IDENT: &str = "kernel";

/// Deprecated spelling of [`KERNEL_CONTEXT_IDENT`]. Still bound for one release
/// so existing kernels keep compiling; every use emits a rename diagnostic.
pub const GPU_CONTEXT_DEPRECATED_IDENT: &str = "gpu_context";

/// Canonical class name for the compiler-builtin `FrameInput` struct made
/// available inside `gpu frame` bodies. Exposes per-frame host input read
/// from a uniform block written by the host each frame.
pub const FRAME_INPUT_TYPE_NAME: &str = "FrameInput";

/// Canonical implicit identifier bound to the per-frame input context inside
/// a `gpu frame` body. Exposes `time`/`dt`/`index`/`mouse_x`/`mouse_y`/
/// `mouse_down`/`drag_dx`/`drag_dy`/`wheel`/`clicked`/`double_clicked`.
pub const FRAME_INPUT_IDENT: &str = "frame";

/// Descriptor for a single frame input field.
#[derive(Debug, Clone, Copy)]
pub struct FrameInputFieldDef {
    pub name: &'static str,
    pub kind: FrameFieldKind,
}

/// Discriminates the wire type of a frame input field in the uniform buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFieldKind {
    F32,
    Int,
    Bool,
}

/// Canonical descriptor of the 11 frame input fields, ordered as they appear
/// in the uniform block (offsets 0–40). Single source of truth for:
/// - Type checker builtin FrameInput struct field definitions
/// - MIR lowering parameter registration (f0..f10)
/// - WGSL manifest emission (buffer layout)
pub const FRAME_INPUT_FIELDS: &[FrameInputFieldDef] = &[
    FrameInputFieldDef {
        name: "time",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "dt",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "index",
        kind: FrameFieldKind::Int,
    },
    FrameInputFieldDef {
        name: "mouse_x",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "mouse_y",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "mouse_down",
        kind: FrameFieldKind::Bool,
    },
    FrameInputFieldDef {
        name: "drag_dx",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "drag_dy",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "wheel",
        kind: FrameFieldKind::F32,
    },
    FrameInputFieldDef {
        name: "clicked",
        kind: FrameFieldKind::Bool,
    },
    FrameInputFieldDef {
        name: "double_clicked",
        kind: FrameFieldKind::Bool,
    },
];

/// Returns the reserved variable_map key for frame input parameter `idx` (0..11).
/// Uses a prefix that cannot be a valid Miri identifier (contains `$`) to prevent
/// collision with user-captured variables.
pub fn frame_input_param_key(idx: usize) -> String {
    format!("$frame${}", idx)
}

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

/// Resolves a type expression to a `TypeKind`, covering forms produced by the
/// pipeline: a `Type(...)` node (post-normalization), a bare identifier, or a
/// literal identifier.
///
/// Used to validate collection element types without requiring a full type
/// resolution pass. Returns the resolved `TypeKind` if the expression is a type
/// identifier; returns `None` otherwise.
pub fn resolve_element_type_kind(expr: &crate::ast::expression::Expression) -> Option<TypeKind> {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::literal::Literal;

    let name = match &expr.node {
        ExpressionKind::Type(inner, _) => return Some(inner.kind.clone()),
        ExpressionKind::Identifier(name, _) => name.as_str(),
        ExpressionKind::Literal(Literal::Identifier(name)) => name.as_str(),
        _ => return None,
    };
    match name {
        "int" => Some(TypeKind::Int),
        "i64" => Some(TypeKind::I64),
        "i32" => Some(TypeKind::I32),
        "i16" => Some(TypeKind::I16),
        "i8" => Some(TypeKind::I8),
        "u64" => Some(TypeKind::U64),
        "u32" => Some(TypeKind::U32),
        "u16" => Some(TypeKind::U16),
        "u8" => Some(TypeKind::U8),
        "i128" => Some(TypeKind::I128),
        "u128" => Some(TypeKind::U128),
        "f32" => Some(TypeKind::F32),
        "f64" => Some(TypeKind::F64),
        _ => None,
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

/// Returns the dimension (2, 3, or 4) of a compiler-known vector type name,
/// or `None` if the name is not a vector type.
///
/// Used to canonically recognize Vec2, Vec3, and Vec4 without string literals
/// scattered across the compiler. The constants [`VEC2_TYPE_NAME`], [`VEC3_TYPE_NAME`],
/// and [`VEC4_TYPE_NAME`] are the single source of truth.
pub fn vec_dim(name: &str) -> Option<u8> {
    match name {
        VEC2_TYPE_NAME => Some(2),
        VEC3_TYPE_NAME => Some(3),
        VEC4_TYPE_NAME => Some(4),
        _ => None,
    }
}

/// Maps a Miri type kind to its WGSL scalar type name.
///
/// This is the single source of truth for the Miri → WGSL scalar mapping,
/// ensuring consistency across the pipeline (web manifest generation) and
/// codegen (WGSL type emission). The mapping respects Miri's type narrowing:
/// - Narrower integer types map to i32/u32 (WGSL minimum widths)
/// - Miri's default int/float map to i64/f64
///
/// # Returns
/// A static string slice (never allocates) naming the WGSL scalar, or `None`
/// if the type is not a scalar (collections, tuples, etc.).
pub fn wgsl_scalar_name(kind: &TypeKind) -> Option<&'static str> {
    match kind {
        TypeKind::I32 | TypeKind::I8 | TypeKind::I16 => Some("i32"),
        TypeKind::U32 | TypeKind::U8 | TypeKind::U16 => Some("u32"),
        TypeKind::F32 => Some("f32"),
        TypeKind::Int => Some("i32"), // Browser-portable: no i64
        TypeKind::I64 => Some("i64"), // Explicit i64 still uses i64
        TypeKind::U64 => Some("u64"),
        TypeKind::Float | TypeKind::F64 => Some("f64"),
        _ => None,
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
