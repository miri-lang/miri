// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::error::syntax::Span;
use std::fmt;

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
    /// Symbol type.
    Symbol,
    /// List type (e.g., `[i32]`).
    List(Box<Expression>),
    /// Array type (e.g., `[i32; 4]`).
    Array(Box<Expression>, Box<Expression>),
    /// Map type (e.g., `{string: i32}`).
    Map(Box<Expression>, Box<Expression>),
    /// Tuple type (e.g., `(i32, string)`).
    Tuple(Vec<Expression>),
    /// Set type (e.g., `{i32}`).
    Set(Box<Expression>),
    /// Result type (e.g., `result<i32, string>`).
    Result(Box<Expression>, Box<Expression>),
    /// Future type (e.g., `future<i32>`).
    Future(Box<Expression>),
    /// Function type (e.g., `fn<T>(x int) float`).
    Function(
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
    ),

    /// Generic type (e.g., `T extends SomeClass`).
    Generic(String, Option<Box<Type>>, TypeDeclarationKind),

    /// Custom type (e.g., struct name).
    Custom(String, Option<Vec<Expression>>),
    /// Metatype (type of a type).
    Meta(Box<Type>),
    /// Nullable type wrapper.
    Nullable(Box<Type>),
    /// Void type.
    Void,
    /// Error type (for type checking).
    Error,
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
            TypeKind::String => write!(f, "string"),
            TypeKind::Boolean => write!(f, "boolean"),
            TypeKind::Symbol => write!(f, "symbol"),
            TypeKind::List(inner) => write!(f, "list({})", inner.node),
            TypeKind::Array(inner, size) => write!(f, "array({}, {})", inner.node, size.node),
            TypeKind::Map(k, v) => write!(f, "map({}, {})", k.node, v.node),
            TypeKind::Tuple(elements) => {
                write!(f, "tuple(")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e.node)?;
                }
                write!(f, ")")
            }
            TypeKind::Set(inner) => write!(f, "set({})", inner.node),
            TypeKind::Result(ok, err) => write!(f, "result({}, {})", ok.node, err.node),
            TypeKind::Future(inner) => write!(f, "future({})", inner.node),
            TypeKind::Function(_, params, ret) => {
                write!(f, "function(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p.typ.node)?;
                }
                write!(f, ")")?;
                if let Some(ret) = ret {
                    write!(f, " -> {}", ret.node)?;
                }
                Ok(())
            }
            TypeKind::Generic(name, _, _) => write!(f, "{}", name),
            TypeKind::Custom(name, args) => {
                write!(f, "{}", name)?;
                if let Some(args) = args {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg.node)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            TypeKind::Meta(inner) => write!(f, "meta({})", inner),
            TypeKind::Nullable(inner) => write!(f, "nullable({})", inner),
            TypeKind::Void => write!(f, "void"),
            TypeKind::Error => write!(f, "error"),
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
