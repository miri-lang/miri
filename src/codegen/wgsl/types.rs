// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL type-name resolution from MIR/AST type kinds.

use crate::ast::expression::ExpressionKind;
use crate::ast::types::{vec_dim, TypeKind};
use crate::error::CodegenError;

/// WGSL scalar types representable in a compute shader.
///
/// `I64`/`U64`/`F64` require host wgpu features (`SHADER_INT64`/`SHADER_F64`)
/// and naga validator capabilities (`SHADER_INT64`/`FLOAT64`) at the launch
/// site. The emitter and the GPU runtime cooperate so an adapter that lacks
/// the matching feature fails the dispatch with `UnsupportedScalar` instead
/// of silently truncating element widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgslScalar {
    I32,
    U32,
    F32,
    Bool,
    I64,
    U64,
    F64,
}

impl WgslScalar {
    /// WGSL source spelling for this scalar.
    pub fn name(self) -> &'static str {
        match self {
            WgslScalar::I32 => "i32",
            WgslScalar::U32 => "u32",
            WgslScalar::F32 => "f32",
            WgslScalar::Bool => "bool",
            WgslScalar::I64 => "i64",
            WgslScalar::U64 => "u64",
            WgslScalar::F64 => "f64",
        }
    }
}

/// Map a scalar MIR/AST type kind to its WGSL scalar representation.
///
/// For browser portability (WebGPU/Tint has no 64-bit int support),
/// Miri's default `Int` maps to WGSL `i32` (not i64). The runtime marshals
/// host i64 buffers ↔ device i32 buffers at launch/readback boundaries.
/// Fixed-width types keep their declared widths (`I32` → `i32`, `I64` → `i64`
/// for CPU-only code). Default `Float` still maps to WGSL `f64`.
/// Not all browsers support WGSL f64; F32 buffers stay f32 unchanged.
///
/// Returns `Err(CodegenError::Internal)` for non-scalar inputs; callers wrap
/// pointer/buffer types in `array<T>` themselves.
pub fn scalar(kind: &TypeKind) -> Result<WgslScalar, CodegenError> {
    match kind {
        TypeKind::I32 | TypeKind::I8 | TypeKind::I16 => Ok(WgslScalar::I32),
        TypeKind::U32 | TypeKind::U8 | TypeKind::U16 => Ok(WgslScalar::U32),
        TypeKind::F32 => Ok(WgslScalar::F32),
        TypeKind::Boolean => Ok(WgslScalar::Bool),
        TypeKind::Int => Ok(WgslScalar::I32), // Browser-portable: no i64
        TypeKind::I64 => Ok(WgslScalar::I64), // Explicit i64 still uses i64
        TypeKind::U64 => Ok(WgslScalar::U64),
        TypeKind::Float | TypeKind::F64 => Ok(WgslScalar::F64),
        TypeKind::I128
        | TypeKind::U128
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

/// Map a vector type kind (Vec2, Vec3, Vec4) to its WGSL vector type spelling.
///
/// Returns `None` for non-vector types. The element type must be a scalar
/// (f32, i32, or u32); f64/i64/u64 widths are rejected at the launch site.
pub fn vector_type(kind: &TypeKind) -> Option<String> {
    match kind {
        TypeKind::Custom(name, Some(args)) => {
            let dim = vec_dim(name)?;
            let first_arg = args.first()?;
            let elem_ty = match &first_arg.node {
                ExpressionKind::Type(ty, _) => ty,
                _ => return None,
            };

            let elem_scalar = scalar(&elem_ty.kind).ok()?;
            Some(format!("vec{}<{}>", dim, elem_scalar.name()))
        }
        _ => None,
    }
}

/// Map a field index to a WGSL vector swizzle character for Vec types.
///
/// Returns the swizzle character (x, y, z, or w) if the type is a vector,
/// otherwise returns `None` to signal use of numeric field access.
pub fn vector_swizzle(kind: &TypeKind, field_idx: usize) -> Option<char> {
    if let TypeKind::Custom(name, _) = kind {
        vec_dim(name).and_then(|dim| {
            debug_assert!(
                field_idx < dim as usize,
                "vector swizzle field index {} out of bounds for dimension {}",
                field_idx,
                dim
            );
            match field_idx {
                0 => Some('x'),
                1 => Some('y'),
                2 => Some('z'),
                3 => Some('w'),
                _ => None,
            }
        })
    } else {
        None
    }
}

/// Extract the element type spelling from a buffer-like collection type.
///
/// Accepts canonical `TypeKind::List(elem)` and `TypeKind::Array(elem, _)` as
/// well as the post-resolution `TypeKind::Custom(name, Some([elem, ...]))`
/// shape that array literals carry through the pipeline. The accepted `name`s
/// are looked up via [`BuiltinCollectionKind::from_name`] so this dispatch
/// never hard-codes stdlib name strings.
pub fn buffer_element(kind: &TypeKind) -> Result<WgslScalar, CodegenError> {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::types::BuiltinCollectionKind;
    let elem_expr = match kind {
        TypeKind::List(elem) => elem,
        TypeKind::Array(elem, _) => elem,
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array) | Some(BuiltinCollectionKind::List)
            ) =>
        {
            args.first().ok_or_else(|| {
                CodegenError::Internal(format!(
                    "WGSL backend: buffer parameter {} missing element type argument",
                    name
                ))
            })?
        }
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
        | ExpressionKind::Cast(..)
        | ExpressionKind::Block(..) => Err(CodegenError::Internal(
            "WGSL backend: unresolved buffer element type expression".into(),
        )),
    }
}
