// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL type-name resolution from MIR/AST type kinds.

use crate::ast::types::TypeKind;
use crate::error::CodegenError;

/// WGSL scalar types representable in a compute shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgslScalar {
    I32,
    U32,
    F32,
    Bool,
}

impl WgslScalar {
    /// WGSL source spelling for this scalar.
    pub fn name(self) -> &'static str {
        match self {
            WgslScalar::I32 => "i32",
            WgslScalar::U32 => "u32",
            WgslScalar::F32 => "f32",
            WgslScalar::Bool => "bool",
        }
    }

    /// WGSL literal for the zero / default value of this scalar.
    pub fn zero_literal(self) -> &'static str {
        match self {
            WgslScalar::I32 => "0",
            WgslScalar::U32 => "0u",
            WgslScalar::F32 => "0.0",
            WgslScalar::Bool => "false",
        }
    }
}

/// Map a scalar MIR/AST type kind to its WGSL scalar representation.
///
/// Returns `Err(CodegenError::Internal)` for non-scalar inputs; callers wrap
/// pointer/buffer types in `array<T>` themselves.
pub fn scalar(kind: &TypeKind) -> Result<WgslScalar, CodegenError> {
    match kind {
        TypeKind::I32 | TypeKind::I8 | TypeKind::I16 | TypeKind::Int => Ok(WgslScalar::I32),
        TypeKind::U32 | TypeKind::U8 | TypeKind::U16 => Ok(WgslScalar::U32),
        TypeKind::F32 | TypeKind::Float => Ok(WgslScalar::F32),
        TypeKind::Boolean => Ok(WgslScalar::Bool),
        TypeKind::I64
        | TypeKind::I128
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Void
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Error
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_) => Err(CodegenError::Internal(format!(
            "WGSL backend cannot represent type {:?} as a scalar",
            kind
        ))),
    }
}

/// Extract the element type spelling from a buffer-like collection type.
pub fn buffer_element(kind: &TypeKind) -> Result<WgslScalar, CodegenError> {
    use crate::ast::expression::ExpressionKind;
    let elem_expr = match kind {
        TypeKind::List(elem) => elem,
        TypeKind::Array(elem, _) => elem,
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
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => {
            return Err(CodegenError::Internal(format!(
                "WGSL backend: buffer parameter has non-collection type {:?}",
                kind
            )));
        }
    };
    match &elem_expr.node {
        ExpressionKind::Type(inner, _) => scalar(&inner.kind),
        ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::Binary(..)
        | ExpressionKind::Logical(..)
        | ExpressionKind::Unary(..)
        | ExpressionKind::Assignment(..)
        | ExpressionKind::Conditional(..)
        | ExpressionKind::Range(..)
        | ExpressionKind::Guard(..)
        | ExpressionKind::Member(..)
        | ExpressionKind::Index(..)
        | ExpressionKind::Call(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::EnumValue(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Lambda(..)
        | ExpressionKind::List(..)
        | ExpressionKind::Array(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Tuple(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Super
        | ExpressionKind::Block(..) => Err(CodegenError::Internal(
            "WGSL backend: unresolved buffer element type expression".into(),
        )),
    }
}
