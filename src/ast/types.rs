// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

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
    F32,
    F64,
    String,
    Boolean,
    Symbol,
    List(Box<Expression>),                    // [i32]
    Map(Box<Expression>, Box<Expression>),    // {string: i32}
    Tuple(Vec<Expression>),                   // (i32, String)
    Set(Box<Expression>),                     // {i32}
    Result(Box<Expression>, Box<Expression>), // result<i32, String>
    Future(Box<Expression>),                  // future<i32>
    Function(
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
    ), // fn<T>(x int) float

    Generic(String, Option<Box<Type>>, TypeDeclarationKind), // T extends Number

    Custom(String, Option<Vec<Expression>>), // a custom type, e.g., MyStruct<T, U>
    Meta(Box<Type>), // Represents the type of a type itself, e.g. the type of the identifier `Point` is `Meta(Custom("Point"))`
    Nullable(Box<Type>), // Represents a nullable type, e.g., `int?`
    Void,            // Represents void type
    Error,           // Represents a type error
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
            TypeKind::Int => write!(f, "Int"),
            TypeKind::I8 => write!(f, "I8"),
            TypeKind::I16 => write!(f, "I16"),
            TypeKind::I32 => write!(f, "I32"),
            TypeKind::I64 => write!(f, "I64"),
            TypeKind::I128 => write!(f, "I128"),
            TypeKind::U8 => write!(f, "U8"),
            TypeKind::U16 => write!(f, "U16"),
            TypeKind::U32 => write!(f, "U32"),
            TypeKind::U64 => write!(f, "U64"),
            TypeKind::U128 => write!(f, "U128"),
            TypeKind::Float => write!(f, "Float"),
            TypeKind::F32 => write!(f, "F32"),
            TypeKind::F64 => write!(f, "F64"),
            TypeKind::String => write!(f, "String"),
            TypeKind::Boolean => write!(f, "Boolean"),
            TypeKind::Symbol => write!(f, "Symbol"),
            TypeKind::List(inner) => write!(f, "List({})", inner.node),
            TypeKind::Map(k, v) => write!(f, "Map({}, {})", k.node, v.node),
            TypeKind::Tuple(elements) => {
                write!(f, "Tuple(")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e.node)?;
                }
                write!(f, ")")
            }
            TypeKind::Set(inner) => write!(f, "Set({})", inner.node),
            TypeKind::Result(ok, err) => write!(f, "Result({}, {})", ok.node, err.node),
            TypeKind::Future(inner) => write!(f, "Future({})", inner.node),
            TypeKind::Function(_, params, ret) => {
                write!(f, "Function(")?;
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
            TypeKind::Meta(inner) => write!(f, "Meta({})", inner),
            TypeKind::Nullable(inner) => write!(f, "Nullable({})", inner),
            TypeKind::Void => write!(f, "Void"),
            TypeKind::Error => write!(f, "Error"),
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
