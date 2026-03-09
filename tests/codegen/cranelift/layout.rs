// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use cranelift_codegen::ir::types;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::codegen::cranelift::layout::{aggregate_size, field_layout};
use miri::error::syntax::Span;
use miri::type_checker::context::{StructDefinition, TypeDefinition};
use std::collections::{BTreeMap, HashMap};

fn ptr_ty() -> types::Type {
    types::I64
}

fn t(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn create_type_expr(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(t(kind)), false),
        span: Span::default(),
    }
}

#[test]
fn test_field_layout_tuple() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I64),
    ]);

    // Field 0: I8
    // Offset should be 0, type I8
    let (offset, ty) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, types::I8);

    // Field 1: I32
    // Alignment is 4, so offset after I8 (1 byte) is padded to 4.
    let (offset, ty) = field_layout(&tuple_kind, 1, &type_defs, ptr);
    assert_eq!(offset, 4);
    assert_eq!(ty, types::I32);

    // Field 2: I64
    // Alignment is 8. After I32 (offset 4+4=8), it's already aligned to 8.
    let (offset, ty) = field_layout(&tuple_kind, 2, &type_defs, ptr);
    assert_eq!(offset, 8);
    assert_eq!(ty, types::I64);
}

#[test]
fn test_aggregate_size_tuple() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I64),
    ]);

    // Offsets: I8 at 0, I32 at 4, I64 at 8.
    // Total physical size before final alignment = 8 + 8 = 16.
    // Max alignment is 8.
    // Final size = 16.
    let size = aggregate_size(&tuple_kind, &type_defs, ptr);
    assert_eq!(size, 16);
}

#[test]
fn test_field_layout_struct() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = StructDefinition {
        fields: vec![
            (
                "a".to_string(),
                t(TypeKind::I16),
                miri::ast::MemberVisibility::Public,
            ),
            (
                "b".to_string(),
                t(TypeKind::F64),
                miri::ast::MemberVisibility::Public,
            ),
            (
                "c".to_string(),
                t(TypeKind::I64),
                miri::ast::MemberVisibility::Public,
            ),
        ],
        generics: None,
        module: String::new(),
    };
    type_defs.insert("MyStruct".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("MyStruct".to_string(), None);

    // Field 0: I16 (offset 0)
    let (offset, ty) = field_layout(&custom_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, types::I16);

    // Field 1: F64 (aligned to 8, offset 2 -> 8)
    let (offset, ty) = field_layout(&custom_kind, 1, &type_defs, ptr);
    assert_eq!(offset, 8);
    assert_eq!(ty, types::F64);

    // Field 2: I64 (aligned to 8, offset 8+8=16 -> 16)
    let (offset, ty) = field_layout(&custom_kind, 2, &type_defs, ptr);
    assert_eq!(offset, 16);
    assert_eq!(ty, types::I64);
}

#[test]
fn test_aggregate_size_struct() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = StructDefinition {
        fields: vec![
            (
                "a".to_string(),
                t(TypeKind::I16),
                miri::ast::MemberVisibility::Public,
            ),
            (
                "b".to_string(),
                t(TypeKind::F64),
                miri::ast::MemberVisibility::Public,
            ),
            (
                "c".to_string(),
                t(TypeKind::I64),
                miri::ast::MemberVisibility::Public,
            ),
        ],
        generics: None,
        module: String::new(),
    };
    type_defs.insert("MyStruct".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("MyStruct".to_string(), None);

    // Sizes: I16 (2), F64 (8), I64 (8)
    // Max align: 8.
    // Layout: 0..2 (I16), 8..16 (F64), 16..24 (I64).
    // Final size = 24.
    let size = aggregate_size(&custom_kind, &type_defs, ptr);
    assert_eq!(size, 24);
}

#[test]
fn test_aggregate_size_class_and_trait() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    type_defs.insert(
        "MyClass".to_string(),
        TypeDefinition::Class(miri::type_checker::context::ClassDefinition {
            name: "MyClass".to_string(),
            generics: None,
            base_class: None,
            traits: vec![],
            fields: BTreeMap::new(),
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
        }),
    );

    type_defs.insert(
        "MyTrait".to_string(),
        TypeDefinition::Trait(miri::type_checker::context::TraitDefinition {
            name: "MyTrait".to_string(),
            generics: None,
            parent_traits: vec![],
            methods: BTreeMap::new(),
            module: String::new(),
        }),
    );

    let class_kind = TypeKind::Custom("MyClass".to_string(), None);
    let trait_kind = TypeKind::Custom("MyTrait".to_string(), None);

    // Classes and Traits are represented as pointers locally (ptr_size)
    assert_eq!(aggregate_size(&class_kind, &type_defs, ptr), ptr.bytes());
    assert_eq!(field_layout(&class_kind, 0, &type_defs, ptr).0, 0);

    assert_eq!(aggregate_size(&trait_kind, &type_defs, ptr), ptr.bytes());
}

#[test]
fn test_aggregate_size_enum() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let mut variants = BTreeMap::new();
    variants.insert("None".to_string(), vec![]);
    // 2 fields means payload size is 2 * ptr_size (enum payloads use pointer-sized fields locally)
    variants.insert("Some".to_string(), vec![t(TypeKind::I32), t(TypeKind::I32)]);

    type_defs.insert(
        "MyEnum".to_string(),
        TypeDefinition::Enum(miri::type_checker::context::EnumDefinition {
            variants,
            generics: None,
        }),
    );

    let enum_kind = TypeKind::Custom("MyEnum".to_string(), None);

    // Enum layout: Discriminant (ptr_size) + max payload (2 * ptr_size) = 3 * ptr_size
    let expected_size = 3 * ptr.bytes();
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), expected_size);

    // Field 0: discriminant (offset 0)
    assert_eq!(field_layout(&enum_kind, 0, &type_defs, ptr).0, 0);

    // Field 1: payload field 1 (offset ptr_size)
    assert_eq!(
        field_layout(&enum_kind, 1, &type_defs, ptr).0,
        ptr.bytes() as i32
    );

    // Field 2: payload field 2 (offset ptr_size + ptr_size)
    assert_eq!(
        field_layout(&enum_kind, 2, &type_defs, ptr).0,
        2 * (ptr.bytes() as i32)
    );
}
