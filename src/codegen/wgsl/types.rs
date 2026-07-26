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
    F16,
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
            WgslScalar::F16 => "f16",
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
        TypeKind::F16 => Ok(WgslScalar::F16),
        TypeKind::F32 => Ok(WgslScalar::F32),
        TypeKind::Boolean => Ok(WgslScalar::Bool),
        TypeKind::Int => Ok(WgslScalar::I32), // Browser-portable: no i64
        TypeKind::I64 => Ok(WgslScalar::I64), // Explicit i64 still uses i64
        TypeKind::U64 => Ok(WgslScalar::U64),
        TypeKind::Float | TypeKind::F64 => Ok(WgslScalar::F64),
        // Atomic<u32> and Atomic<i32> unwrap to their inner scalar types
        TypeKind::Custom(name, Some(args)) if name == crate::ast::types::ATOMIC_TYPE_NAME => {
            if args.len() == 1 {
                if let ExpressionKind::Type(inner_ty, _) = &args[0].node {
                    return scalar(&inner_ty.kind);
                }
            }
            Err(CodegenError::Internal(format!(
                "WGSL backend cannot represent type {:?} as a scalar",
                kind
            )))
        }
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
    component_scalar(buffer_element_inner_kind(kind)?)
}

/// Full WGSL element-type spelling for a buffer-like collection: a scalar
/// (`f32`) or, for an inline vector element, the vector form (`vec3<f32>`).
///
/// Used for the `array<...>` storage-buffer declaration. The plain
/// [`buffer_element`] returns only the component scalar (e.g. the `f32` of a
/// `vec3<f32>` element) for callers that reason about component width.
pub fn buffer_element_typename(kind: &TypeKind) -> Result<String, CodegenError> {
    let inner = buffer_element_inner_kind(kind)?;
    match vector_type(inner) {
        Some(vec_spelling) => Ok(vec_spelling),
        None => Ok(component_scalar(inner)?.name().to_string()),
    }
}

/// Resolve the component scalar of an element kind: the vector component for a
/// `VecN<T>` element, otherwise the scalar itself.
fn component_scalar(inner: &TypeKind) -> Result<WgslScalar, CodegenError> {
    if let TypeKind::Custom(name, Some(args)) = inner {
        if vec_dim(name).is_some() {
            if let Some(ExpressionKind::Type(elem, _)) = args.first().map(|a| &a.node) {
                return scalar(&elem.kind);
            }
        }
    }
    scalar(inner)
}

/// Extract the element type-kind from a buffer-like collection type (the inner
/// `T` of `Array<T, N>` / `List<T>` and their post-resolution `Custom` forms).
fn buffer_element_inner_kind(kind: &TypeKind) -> Result<&TypeKind, CodegenError> {
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
        | TypeKind::F16
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
        ExpressionKind::Type(inner, _) => Ok(&inner.kind),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expression::Expression;
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::types::{
        BuiltinCollectionKind, Type, ATOMIC_TYPE_NAME, STRING_TYPE_NAME, VEC2_TYPE_NAME,
        VEC3_TYPE_NAME, VEC4_TYPE_NAME,
    };
    use crate::error::syntax::Span;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn type_arg(kind: TypeKind) -> Expression {
        let ty = Type::new(kind, span());
        Expression::new(0, ExpressionKind::Type(Box::new(ty), false), span())
    }

    fn literal_arg() -> Expression {
        Expression::new(
            0,
            ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I32(4))),
            span(),
        )
    }

    fn generic(name: &str, args: Vec<TypeKind>) -> TypeKind {
        TypeKind::Custom(
            name.to_string(),
            Some(args.into_iter().map(type_arg).collect()),
        )
    }

    fn vec_of(name: &str, elem: TypeKind) -> TypeKind {
        generic(name, vec![elem])
    }

    fn list_of(elem: TypeKind) -> TypeKind {
        TypeKind::List(Box::new(type_arg(elem)))
    }

    fn array_of(elem: TypeKind) -> TypeKind {
        TypeKind::Array(Box::new(type_arg(elem)), Box::new(literal_arg()))
    }

    /// The WGSL scalar for `kind`, failing the test if the mapping rejects it.
    fn wgsl_scalar(kind: &TypeKind) -> WgslScalar {
        scalar(kind).unwrap_or_else(|e| panic!("{kind:?} should be a WGSL scalar: {e:?}"))
    }

    fn buffer_scalar(kind: &TypeKind) -> WgslScalar {
        buffer_element(kind).unwrap_or_else(|e| panic!("{kind:?} should be a buffer: {e:?}"))
    }

    fn buffer_typename(kind: &TypeKind) -> String {
        buffer_element_typename(kind)
            .unwrap_or_else(|e| panic!("{kind:?} should be a buffer: {e:?}"))
    }

    #[test]
    fn test_narrow_signed_kinds_widen_to_the_i32_lane() {
        for kind in [TypeKind::I32, TypeKind::I16, TypeKind::I8] {
            assert_eq!(wgsl_scalar(&kind), WgslScalar::I32, "{kind:?}");
        }
    }

    #[test]
    fn test_narrow_unsigned_kinds_widen_to_the_u32_lane() {
        for kind in [TypeKind::U32, TypeKind::U16, TypeKind::U8] {
            assert_eq!(wgsl_scalar(&kind), WgslScalar::U32, "{kind:?}");
        }
    }

    #[test]
    fn test_default_int_maps_to_i32_for_browser_portability() {
        // WebGPU/Tint has no 64-bit ints, so the default `int` must not reach
        // WGSL as i64 — the runtime marshals host i64 buffers to device i32.
        assert_eq!(wgsl_scalar(&TypeKind::Int), WgslScalar::I32);
        assert_eq!(wgsl_scalar(&TypeKind::I64), WgslScalar::I64);
        assert_eq!(wgsl_scalar(&TypeKind::U64), WgslScalar::U64);
    }

    #[test]
    fn test_float_kinds_keep_their_declared_widths() {
        assert_eq!(wgsl_scalar(&TypeKind::F16), WgslScalar::F16);
        assert_eq!(wgsl_scalar(&TypeKind::F32), WgslScalar::F32);
        assert_eq!(wgsl_scalar(&TypeKind::F64), WgslScalar::F64);
        assert_eq!(wgsl_scalar(&TypeKind::Float), WgslScalar::F64);
    }

    #[test]
    fn test_boolean_maps_to_the_wgsl_bool() {
        assert_eq!(wgsl_scalar(&TypeKind::Boolean), WgslScalar::Bool);
    }

    #[test]
    fn test_atomic_unwraps_to_its_inner_scalar() {
        assert_eq!(
            wgsl_scalar(&vec_of(ATOMIC_TYPE_NAME, TypeKind::U32)),
            WgslScalar::U32
        );
        assert_eq!(
            wgsl_scalar(&vec_of(ATOMIC_TYPE_NAME, TypeKind::I32)),
            WgslScalar::I32
        );
    }

    #[test]
    fn test_atomic_over_a_non_scalar_is_rejected() {
        assert!(scalar(&vec_of(ATOMIC_TYPE_NAME, TypeKind::String)).is_err());
    }

    #[test]
    fn test_atomic_without_exactly_one_type_argument_is_rejected() {
        assert!(scalar(&generic(ATOMIC_TYPE_NAME, vec![])).is_err());
        assert!(scalar(&generic(
            ATOMIC_TYPE_NAME,
            vec![TypeKind::U32, TypeKind::U32]
        ))
        .is_err());
        assert!(scalar(&TypeKind::Custom(ATOMIC_TYPE_NAME.to_string(), None)).is_err());
    }

    #[test]
    fn test_non_scalar_kinds_are_rejected_with_the_kind_named() {
        let unrepresentable = TypeKind::String;
        let err = scalar(&unrepresentable).expect_err("a string is not a WGSL scalar");
        assert!(
            format!("{err:?}").contains(&format!("{unrepresentable:?}")),
            "the error should name the offending kind, got: {err:?}"
        );
        for kind in [
            TypeKind::Void,
            TypeKind::I128,
            TypeKind::U128,
            TypeKind::RawPtr,
            list_of(TypeKind::F32),
            TypeKind::Custom("Widget".to_string(), None),
        ] {
            assert!(scalar(&kind).is_err(), "{kind:?} must not be a scalar");
        }
    }

    #[test]
    fn test_scalar_spellings_are_the_wgsl_keywords() {
        assert_eq!(WgslScalar::I32.name(), "i32");
        assert_eq!(WgslScalar::U32.name(), "u32");
        assert_eq!(WgslScalar::F16.name(), "f16");
        assert_eq!(WgslScalar::F32.name(), "f32");
        assert_eq!(WgslScalar::Bool.name(), "bool");
        assert_eq!(WgslScalar::I64.name(), "i64");
        assert_eq!(WgslScalar::U64.name(), "u64");
        assert_eq!(WgslScalar::F64.name(), "f64");
    }

    #[test]
    fn test_vector_types_spell_their_dimension_and_component() {
        assert_eq!(
            vector_type(&vec_of(VEC2_TYPE_NAME, TypeKind::F32)).as_deref(),
            Some("vec2<f32>")
        );
        assert_eq!(
            vector_type(&vec_of(VEC3_TYPE_NAME, TypeKind::F32)).as_deref(),
            Some("vec3<f32>")
        );
        assert_eq!(
            vector_type(&vec_of(VEC4_TYPE_NAME, TypeKind::U32)).as_deref(),
            Some("vec4<u32>")
        );
    }

    #[test]
    fn test_vector_type_is_none_for_anything_that_is_not_a_vector() {
        assert_eq!(vector_type(&TypeKind::F32), None);
        assert_eq!(vector_type(&generic("Widget", vec![TypeKind::F32])), None);
        assert_eq!(
            vector_type(&TypeKind::Custom(VEC3_TYPE_NAME.to_string(), None)),
            None
        );
        assert_eq!(vector_type(&generic(VEC3_TYPE_NAME, vec![])), None);
    }

    #[test]
    fn test_vector_type_is_none_when_the_component_is_unresolved_or_unrepresentable() {
        let unresolved = TypeKind::Custom(VEC3_TYPE_NAME.to_string(), Some(vec![literal_arg()]));
        assert_eq!(vector_type(&unresolved), None);
        assert_eq!(vector_type(&vec_of(VEC3_TYPE_NAME, TypeKind::String)), None);
    }

    #[test]
    fn test_vector_swizzle_maps_field_order_to_xyzw() {
        let vec4 = vec_of(VEC4_TYPE_NAME, TypeKind::F32);
        assert_eq!(vector_swizzle(&vec4, 0), Some('x'));
        assert_eq!(vector_swizzle(&vec4, 1), Some('y'));
        assert_eq!(vector_swizzle(&vec4, 2), Some('z'));
        assert_eq!(vector_swizzle(&vec4, 3), Some('w'));
    }

    #[test]
    fn test_vector_swizzle_is_none_for_non_vector_types() {
        // A `None` answer routes the caller to numeric field access instead.
        assert_eq!(vector_swizzle(&TypeKind::F32, 0), None);
        assert_eq!(
            vector_swizzle(&TypeKind::Custom("Widget".to_string(), None), 0),
            None
        );
    }

    #[test]
    fn test_buffer_element_reads_through_every_collection_shape() {
        assert_eq!(buffer_scalar(&list_of(TypeKind::F32)), WgslScalar::F32);
        assert_eq!(buffer_scalar(&array_of(TypeKind::I32)), WgslScalar::I32);
        assert_eq!(
            buffer_scalar(&generic(
                BuiltinCollectionKind::List.name(),
                vec![TypeKind::U32]
            )),
            WgslScalar::U32
        );
        assert_eq!(
            buffer_scalar(&generic(
                BuiltinCollectionKind::Array.name(),
                vec![TypeKind::Boolean]
            )),
            WgslScalar::Bool
        );
    }

    #[test]
    fn test_buffer_element_of_a_vector_element_is_its_component_scalar() {
        let buffer = list_of(vec_of(VEC3_TYPE_NAME, TypeKind::F32));
        assert_eq!(buffer_scalar(&buffer), WgslScalar::F32);
    }

    #[test]
    fn test_buffer_element_rejects_non_collection_and_non_buffer_collections() {
        assert!(buffer_element(&TypeKind::F32).is_err());
        assert!(buffer_element(&TypeKind::Tuple(vec![])).is_err());
        assert!(buffer_element(&generic(
            BuiltinCollectionKind::Set.name(),
            vec![TypeKind::F32]
        ))
        .is_err());
        assert!(buffer_element(&generic("Widget", vec![TypeKind::F32])).is_err());
    }

    #[test]
    fn test_buffer_element_rejects_a_collection_with_no_element_argument() {
        let empty = TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![]),
        );
        let err = buffer_element(&empty).expect_err("an Array with no element type is malformed");
        assert!(
            format!("{err:?}").contains("missing element type argument"),
            "got: {err:?}"
        );
    }

    #[test]
    fn test_buffer_element_rejects_an_unresolved_element_expression() {
        let unresolved = TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![literal_arg()]),
        );
        let err = buffer_element(&unresolved).expect_err("a literal is not an element type");
        assert!(
            format!("{err:?}").contains("unresolved buffer element type expression"),
            "got: {err:?}"
        );
    }

    #[test]
    fn test_buffer_element_typename_keeps_a_vector_element_whole() {
        assert_eq!(
            buffer_typename(&list_of(vec_of(VEC3_TYPE_NAME, TypeKind::F32))),
            "vec3<f32>"
        );
        assert_eq!(
            buffer_typename(&array_of(vec_of(VEC2_TYPE_NAME, TypeKind::U32))),
            "vec2<u32>"
        );
    }

    #[test]
    fn test_buffer_element_typename_of_a_scalar_element_is_the_scalar_spelling() {
        assert_eq!(buffer_typename(&list_of(TypeKind::F32)), "f32");
        assert_eq!(buffer_typename(&array_of(TypeKind::Int)), "i32");
    }

    #[test]
    fn test_buffer_element_typename_rejects_an_unrepresentable_element() {
        let strings = list_of(TypeKind::Custom(STRING_TYPE_NAME.to_string(), None));
        assert!(buffer_element_typename(&strings).is_err());
    }
}
