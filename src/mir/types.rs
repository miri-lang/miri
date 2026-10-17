// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-level type representation.
//!
//! [`MirType`] is a resolved, AST-free type used in MIR analysis passes.
//!
//! The main difference from [`crate::ast::types::TypeKind`] is that collection
//! element types are stored as `Box<MirType>` rather than `Box<Expression>`.
//! This eliminates the layering violation where the Perceus pass had to
//! pattern-match on AST [`ExpressionKind`] nodes to determine element types.
//!
//! [`MirType`] is constructed from [`TypeKind`] when a [`LocalDecl`] is created
//! during MIR lowering, so no extra traversal is needed at analysis time.
//!
//! [`ExpressionKind`]: crate::ast::expression::ExpressionKind
//! [`TypeKind`]: crate::ast::types::TypeKind
//! [`LocalDecl`]: crate::mir::LocalDecl

use crate::ast::types::{BuiltinCollectionKind, TypeKind};

/// A resolved, AST-expression-free type for use in MIR analysis.
///
/// All collection element types (e.g. the `T` in `[T]`) are stored as
/// `Box<MirType>` rather than `Box<Expression>`, so analysis passes can
/// traverse the type tree without touching the AST.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MirType {
    Int,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    Float,
    F16,
    F32,
    F64,
    Boolean,
    Void,
    Identifier,
    RawPtr,
    Error,

    /// Heap-allocated via `Box`, but **not** RC-managed (no `alloc_with_rc`
    /// layout).  Included as its own variant so callers can distinguish it
    /// from user-defined types without string matching.
    String,

    /// Dynamic list `[T]`.  Element type resolved from the AST type expression.
    List(Box<MirType>),
    /// Fixed-size array `[T; N]`.  Element type resolved; size is stored for
    /// completeness but not required by RC analysis.
    Array(Box<MirType>),
    /// Hash map `{K: V}`.  Key and value types both resolved.
    Map(Box<MirType>, Box<MirType>),
    /// Hash set `{T}`.  Element type resolved.
    Set(Box<MirType>),
    /// Tuple `(T0, T1, …)`.  Element types resolved.
    Tuple(Vec<MirType>),
    /// Result `result<T, E>`.  Both type arguments resolved.
    Result(Box<MirType>, Box<MirType>),
    /// Optional `T?` or `Option<T>`.  Inner type resolved.
    Option(Box<MirType>),
    /// Async future `future<T>`.  Inner type resolved.
    Future(Box<MirType>),

    /// A named user-defined type (struct, class, or enum).
    /// Generic arguments are not tracked: whether a value is reference counted
    /// is asked of the declared type, which keeps them.
    Custom(String),

    /// A compiler-known vector (`Vec2`, `Vec3`, `Vec4`), written with its
    /// component. A program's own type of the same name, written without one,
    /// is a [`MirType::Custom`] like any other.
    Vector(String),

    /// A heap-allocated closure (function value with a captured environment).
    /// Closure structs are RC-managed: [malloc_ptr][RC][fn_ptr][cap0][cap1]...
    /// The closure local holds `payload_ptr` (the word past the RC header).
    Function,

    /// An unresolved generic type parameter (e.g. `T`, `K`, `V`).
    /// Generic parameters are never concrete heap objects.
    Generic,

    /// A type that could not be resolved during MIR construction. A
    /// projection through one resolves to nothing, so it gets no IncRef or
    /// DecRef.
    Unknown,
}

impl MirType {
    /// Converts an AST [`TypeKind`] into a [`MirType`], resolving all
    /// collection element-type expressions into `MirType` values.
    ///
    /// This is called once per [`LocalDecl`] at MIR construction time.
    ///
    /// [`LocalDecl`]: crate::mir::LocalDecl
    pub fn from_type_kind(kind: &TypeKind) -> Self {
        match kind {
            TypeKind::Int => MirType::Int,
            TypeKind::I8 => MirType::I8,
            TypeKind::I16 => MirType::I16,
            TypeKind::I32 => MirType::I32,
            TypeKind::I64 => MirType::I64,
            TypeKind::I128 => MirType::I128,
            TypeKind::U8 => MirType::U8,
            TypeKind::U16 => MirType::U16,
            TypeKind::U32 => MirType::U32,
            TypeKind::U64 => MirType::U64,
            TypeKind::U128 => MirType::U128,
            TypeKind::Float => MirType::Float,
            TypeKind::F16 => MirType::F16,
            TypeKind::F32 => MirType::F32,
            TypeKind::F64 => MirType::F64,
            TypeKind::String => MirType::String,
            TypeKind::Boolean => MirType::Boolean,
            TypeKind::Void => MirType::Void,
            TypeKind::Identifier => MirType::Identifier,
            TypeKind::RawPtr => MirType::RawPtr,
            TypeKind::Error | TypeKind::OneOf(_) => MirType::Error,
            // Canonical collection variants are normalized to Custom before MIR lowering.
            // They are handled below in the Custom arm.
            TypeKind::List(elem) => MirType::List(Box::new(Self::from_expr(elem))),
            TypeKind::Array(elem, _size) => MirType::Array(Box::new(Self::from_expr(elem))),
            TypeKind::Map(k, v) => {
                MirType::Map(Box::new(Self::from_expr(k)), Box::new(Self::from_expr(v)))
            }
            TypeKind::Set(elem) => MirType::Set(Box::new(Self::from_expr(elem))),
            TypeKind::Tuple(elems) => MirType::Tuple(elems.iter().map(Self::from_expr).collect()),
            TypeKind::Result(ok, err) => MirType::Result(
                Box::new(Self::from_expr(ok)),
                Box::new(Self::from_expr(err)),
            ),
            TypeKind::Option(inner) => MirType::Option(Box::new(Self::from_type_kind(&inner.kind))),
            TypeKind::Future(inner) => MirType::Future(Box::new(Self::from_expr(inner))),
            // Generic type parameters — never concrete managed types.
            TypeKind::Generic(_, _, _) => MirType::Generic,
            // Closures are heap-allocated and RC-managed.
            TypeKind::Function(_) => MirType::Function,
            TypeKind::Custom(name, args) => Self::from_custom(name, args.as_deref()),
            // Meta and linear types are not involved in RC management.
            TypeKind::Meta(_) | TypeKind::Linear(_) => MirType::Unknown,
        }
    }

    /// Map a `Custom(name, args)` back to a MirType collection variant if the
    /// name canonicalizes to a built-in collection and the args are instantiated,
    /// and the atomic element type to the scalar it is stored as.
    /// When `args` is `None`, the name appears as an unresolved self-reference
    /// inside a stdlib class body — keep it as `MirType::Custom`, an element
    /// value rather than a collection.
    fn from_custom(name: &str, args: Option<&[crate::ast::expression::Expression]>) -> Self {
        // The atomic element type is stored as its scalar component, so it is
        // that scalar to every analysis — a user type sharing the name is not.
        if name == crate::ast::types::ATOMIC_TYPE_NAME {
            if let Some(component) = crate::mir::rc::atomic_component(args) {
                return Self::from_expr(component);
            }
        }
        if crate::ast::types::written_vec_dim(name, args).is_some() {
            return MirType::Vector(name.to_string());
        }
        let Some(builtin) = BuiltinCollectionKind::from_name(name) else {
            return MirType::Custom(name.to_string());
        };
        let Some(args) = args else {
            return MirType::Custom(name.to_string());
        };
        let arg_or_unknown = |idx: usize| {
            args.get(idx)
                .map(Self::from_expr)
                .unwrap_or(MirType::Unknown)
        };
        match builtin {
            BuiltinCollectionKind::List => MirType::List(Box::new(arg_or_unknown(0))),
            BuiltinCollectionKind::Array => MirType::Array(Box::new(arg_or_unknown(0))),
            BuiltinCollectionKind::Map => {
                MirType::Map(Box::new(arg_or_unknown(0)), Box::new(arg_or_unknown(1)))
            }
            BuiltinCollectionKind::Set => MirType::Set(Box::new(arg_or_unknown(0))),
        }
    }

    /// Whether this is a vector type. A vector is stored inline wherever a
    /// collection lays its elements out at their std430 stride, so it is the
    /// one managed type whose element position holds bytes rather than a
    /// pointer.
    ///
    /// Only a vector written with its component is one: a program's own type
    /// named `Vec3` is an ordinary struct, held behind a pointer. Every vector
    /// that reaches MIR has a component with an inline layout: the type checker
    /// refuses the rest where they are written (`validate_vector_component`).
    pub fn is_inline_vector(&self) -> bool {
        matches!(self, MirType::Vector(_))
    }

    /// Resolves a type expression node into a [`MirType`].
    ///
    /// The parser encodes type arguments inside collection type annotations
    /// (e.g. `[T]`, `[T; N]`, `{K: V}`) as [`Expression`] nodes rather than
    /// [`Type`] nodes.  This helper handles the two common forms:
    ///
    /// - `ExpressionKind::Type(ty, is_nullable)` — a fully-formed type node;
    ///   recurse, wrapping in [`MirType::Option`] when the node is nullable.
    ///   `int?` and `Option<int>` name one type, and an element that loses the
    ///   nullable half looks like a bare value the pass need not release.
    /// - `ExpressionKind::Identifier(name, _)` — a bare name like `"String"`,
    ///   `"MyClass"`, or a generic parameter; mapped to `Custom(name)`.
    ///
    /// Any other expression form (e.g. a size literal in `[T; 4]`) maps to
    /// [`MirType::Unknown`].
    ///
    /// [`Expression`]: crate::ast::expression::Expression
    /// [`Type`]: crate::ast::types::Type
    fn from_expr(expr: &crate::ast::expression::Expression) -> Self {
        match &expr.node {
            crate::ast::expression::ExpressionKind::Type(ty, is_nullable) => {
                let resolved = Self::from_type_kind(&ty.kind);
                if *is_nullable {
                    MirType::Option(Box::new(resolved))
                } else {
                    resolved
                }
            }
            crate::ast::expression::ExpressionKind::Identifier(name, _) => {
                // A bare identifier in element-type position is a type name,
                // kept as written; whether it is a class, a generic parameter
                // or a collection's own name is decided where the type table
                // and the body's type parameters are known.
                MirType::Custom(name.clone())
            }
            _ => MirType::Unknown,
        }
    }
}
