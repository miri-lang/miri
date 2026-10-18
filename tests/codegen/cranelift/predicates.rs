// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, VEC2_TYPE_NAME, VEC3_TYPE_NAME, VEC4_TYPE_NAME,
};
use miri::codegen::cranelift::translator::ElementShape;
use miri::codegen::cranelift::FunctionTranslator;
use miri::error::syntax::Span;

fn span() -> Span {
    Span::new(0, 0)
}

fn mk_type(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn mk_expr_type(kind: TypeKind) -> Expression {
    Expression::new(
        0,
        ExpressionKind::Type(Box::new(mk_type(kind)), false),
        span(),
    )
}

#[test]
fn classify_element_shape_scalar_types_are_other() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::I32),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::F64),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Boolean),
        ElementShape::Other
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Void),
        ElementShape::Other
    ));
}

#[test]
fn classify_element_shape_string() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::String),
        ElementShape::String
    ));
}

#[test]
fn classify_element_shape_builtin_collections() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::List(Box::new(mk_expr_type(
            TypeKind::I32
        )))),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Array(
            Box::new(mk_expr_type(TypeKind::I32)),
            Box::new(mk_expr_type(TypeKind::I64))
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Array)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Set(Box::new(mk_expr_type(
            TypeKind::I32
        )))),
        ElementShape::Builtin(BuiltinCollectionKind::Set)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Map(
            Box::new(mk_expr_type(TypeKind::String)),
            Box::new(mk_expr_type(TypeKind::I32))
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Map)
    ));
}

/// A vector written with the component it holds is stored inline and carries no
/// per-element drop callback.
#[test]
fn classify_element_shape_vec_types_are_other() {
    for name in [VEC2_TYPE_NAME, VEC3_TYPE_NAME, VEC4_TYPE_NAME] {
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::Custom(
                name.to_string(),
                Some(vec![mk_expr_type(TypeKind::F32)])
            )),
            ElementShape::Other
        ));
    }
}

/// A declaration reusing a vector's name without the component it holds is an
/// ordinary type, so its elements are released through that type's own thunk.
#[test]
fn classify_element_shape_treats_a_type_reusing_a_vector_name_as_a_user_class() {
    for name in [VEC2_TYPE_NAME, VEC3_TYPE_NAME, VEC4_TYPE_NAME] {
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::Custom(name.to_string(), None)),
            ElementShape::UserClass(found) if found == name
        ));
    }
}

#[test]
fn classify_element_shape_atomic_is_other() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("Atomic".to_string(), None)),
        ElementShape::Other
    ));
}

#[test]
fn classify_element_shape_custom_builtin_normalized() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom(
            "List".to_string(),
            Some(vec![mk_expr_type(TypeKind::I32)])
        )),
        ElementShape::Builtin(BuiltinCollectionKind::List)
    ));
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom(
            "Array".to_string(),
            Some(vec![
                mk_expr_type(TypeKind::F32),
                mk_expr_type(TypeKind::I32)
            ])
        )),
        ElementShape::Builtin(BuiltinCollectionKind::Array)
    ));
}

#[test]
fn classify_element_shape_user_class() {
    assert!(matches!(
        FunctionTranslator::classify_element_shape(&TypeKind::Custom("MyClass".to_string(), None)),
        ElementShape::UserClass("MyClass")
    ));
}

#[test]
fn is_set_type_direct() {
    assert!(FunctionTranslator::is_set_type(&TypeKind::Set(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
}

#[test]
fn is_set_type_normalized_custom() {
    assert!(FunctionTranslator::is_set_type(&TypeKind::Custom(
        "Set".to_string(),
        Some(vec![mk_expr_type(TypeKind::I32)])
    )));
}

#[test]
fn is_set_type_not_other_collections() {
    assert!(!FunctionTranslator::is_set_type(&TypeKind::List(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_set_type_not_scalars() {
    assert!(!FunctionTranslator::is_set_type(&TypeKind::I32));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::String));
    assert!(!FunctionTranslator::is_set_type(&TypeKind::Custom(
        "MyClass".to_string(),
        None
    )));
}

#[test]
fn is_list_type_direct() {
    assert!(FunctionTranslator::is_list_type(&TypeKind::List(Box::new(
        mk_expr_type(TypeKind::I32)
    ))));
}

#[test]
fn is_list_type_normalized() {
    assert!(FunctionTranslator::is_list_type(&TypeKind::Custom(
        "List".to_string(),
        Some(vec![mk_expr_type(TypeKind::I32)])
    )));
}

#[test]
fn is_list_type_not_array() {
    assert!(!FunctionTranslator::is_list_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
}

#[test]
fn is_map_type_direct() {
    assert!(FunctionTranslator::is_map_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_map_type_normalized() {
    assert!(FunctionTranslator::is_map_type(&TypeKind::Custom(
        "Map".to_string(),
        Some(vec![
            mk_expr_type(TypeKind::String),
            mk_expr_type(TypeKind::I32)
        ])
    )));
}

#[test]
fn is_collection_type_all_builtin_collections() {
    assert!(FunctionTranslator::is_collection_type(&TypeKind::List(
        Box::new(mk_expr_type(TypeKind::I32))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Array(
        Box::new(mk_expr_type(TypeKind::I32)),
        Box::new(mk_expr_type(TypeKind::I64))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Set(
        Box::new(mk_expr_type(TypeKind::I32))
    )));
    assert!(FunctionTranslator::is_collection_type(&TypeKind::Map(
        Box::new(mk_expr_type(TypeKind::String)),
        Box::new(mk_expr_type(TypeKind::I32))
    )));
}

#[test]
fn is_collection_type_not_scalars() {
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::I32));
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::String));
    assert!(!FunctionTranslator::is_collection_type(&TypeKind::Boolean));
}

#[test]
fn is_unsigned_type_kind_all_unsigned() {
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U8));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U16));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U32));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U64));
    assert!(FunctionTranslator::is_unsigned_type_kind(&TypeKind::U128));
}

#[test]
fn is_unsigned_type_kind_not_signed() {
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::Int));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I8));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I32));
    assert!(!FunctionTranslator::is_unsigned_type_kind(&TypeKind::I64));
}

#[test]
fn is_integer_kind_signed() {
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::Int));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I8));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I16));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I32));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I64));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::I128));
}

#[test]
fn is_integer_kind_unsigned() {
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U8));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U16));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U32));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U64));
    assert!(FunctionTranslator::is_integer_kind(&TypeKind::U128));
}

#[test]
fn is_integer_kind_not_float() {
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Float));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::F32));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::F64));
}

#[test]
fn is_integer_kind_not_other() {
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::String));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Boolean));
    assert!(!FunctionTranslator::is_integer_kind(&TypeKind::Void));
}

/// The element kind word, decoded against the runtime's bit layout
/// (`element_identity::through_optionals`): the rule in bits 0..8, the boxed
/// depth in bits 8..16, the innermost-nullable flag at bit 16, the
/// slot-address flag at bit 17, and the boxed value's size from bit 18. The
/// runtime opens exactly the boxes the word names, so a bit out of place reads
/// a pointer as a value.
#[test]
fn test_optional_element_kind_matches_the_runtime_bit_layout() {
    use miri::mir::option_layout::OptionalLayers;
    use std::borrow::Cow;
    let node = TypeKind::Custom("Node".to_string(), None);
    let layers = |boxed, innermost_nullable_pointer, value: &TypeKind| OptionalLayers {
        boxed,
        inline: 0,
        innermost_nullable_pointer,
        value: Cow::Owned(value.clone()),
    };
    let string_content = 1;
    let f64_value = 4;
    assert_eq!(
        FunctionTranslator::optional_element_kind(string_content, &layers(0, true, &node), 8),
        Some(1 | (1 << 16) | (8 << 18)),
        "a nullable pointer, no box"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(string_content, &layers(1, true, &node), 8),
        Some(1 | (1 << 8) | (1 << 16) | (8 << 18)),
        "one box around a nullable pointer"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(f64_value, &layers(2, false, &TypeKind::F64), 8),
        Some(4 | (2 << 8) | (8 << 18)),
        "two boxes around a value"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(f64_value, &layers(0, false, &TypeKind::F64), 8),
        None,
        "no optional layer to describe"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(
            f64_value,
            &layers(256, false, &TypeKind::F64),
            8
        ),
        None,
        "a depth past the eight bits the runtime reads"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(0, &layers(1, false, &node), 24),
        Some((1 << 8) | (24 << 18)),
        "one box around a value held as its 24 bytes"
    );
    assert_eq!(
        FunctionTranslator::optional_element_kind(
            f64_value,
            &layers(1, false, &TypeKind::F64),
            129
        ),
        None,
        "a value wider than any value held as its bytes"
    );
}
