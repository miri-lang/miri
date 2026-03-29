// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use cranelift_codegen::ir::types;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::layout::{aggregate_size, field_layout};

use miri::error::syntax::Span;
use miri::type_checker::context::{
    AliasDefinition, ClassDefinition, EnumDefinition, FieldInfo, GenericDefinition,
    StructDefinition, TraitDefinition, TypeDefinition,
};

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

// ═══════════════════════════════════════════════════════════════════════
// Tuple layout
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_tuple_field_layout_with_padding() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    // (I8, I32, I64) — requires alignment padding
    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I64),
    ]);

    // Fields start after the count header (ptr_size = 8 bytes)
    // Field 0: I8 at offset 8
    let (offset, ty) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 8);
    assert_eq!(ty, types::I8);

    // Field 1: I32, alignment 4 → offset 9 padded to 12
    let (offset, ty) = field_layout(&tuple_kind, 1, &type_defs, ptr);
    assert_eq!(offset, 12);
    assert_eq!(ty, types::I32);

    // Field 2: I64, alignment 8 → offset 12+4=16, already aligned
    let (offset, ty) = field_layout(&tuple_kind, 2, &type_defs, ptr);
    assert_eq!(offset, 16);
    assert_eq!(ty, types::I64);
}

#[test]
fn test_tuple_aggregate_size_with_padding() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I64),
    ]);

    // Layout: count@0, I8@8, I32@12, I64@16 → end=24, max_align=8 → final=24
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 24);
}

#[test]
fn test_tuple_single_field() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![create_type_expr(TypeKind::I32)]);

    // Field 0 starts after count header (8 bytes)
    let (offset, ty) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 8);
    assert_eq!(ty, types::I32);

    // Size: count@0 (8 bytes) + I32@8 (4 bytes) = 12, max_align=8 → align_to(12, 8) = 16
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 16);
}

#[test]
fn test_tuple_empty() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![]);

    // Empty tuple: total=ptr_size(8) (count header only), max_align=8 → 8
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 8);
}

#[test]
fn test_tuple_no_padding_needed() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    // (I32, I32) — both 4-byte aligned, no padding gaps
    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I32),
    ]);

    // Fields start after count header (8 bytes)
    let (offset0, _) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    let (offset1, _) = field_layout(&tuple_kind, 1, &type_defs, ptr);
    assert_eq!(offset0, 8);
    assert_eq!(offset1, 12);

    // Total = count@0 (8) + I32@8 (4) + I32@12 (4) = 16, max_align=8 → 16
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 16);
}

#[test]
fn test_tuple_with_pointer_sized_elements() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    // (String, I32, String) — String is ptr-sized (8 bytes on 64-bit)
    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::String),
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::String),
    ]);

    // Fields start after count header (8 bytes)
    // String@8 (8 bytes), I32@16 (4 bytes, align=4, 16 is ok), String@24 (align=8, 20→24)
    let (offset0, ty0) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    assert_eq!(offset0, 8);
    assert_eq!(ty0, ptr);

    let (offset1, ty1) = field_layout(&tuple_kind, 1, &type_defs, ptr);
    assert_eq!(offset1, 16);
    assert_eq!(ty1, types::I32);

    let (offset2, ty2) = field_layout(&tuple_kind, 2, &type_defs, ptr);
    assert_eq!(offset2, 24);
    assert_eq!(ty2, ptr);

    // Total = count@0 (8) + String@8 (8) + I32@16 (4) + String@24 (8) = 32, max_align=8 → 32
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 32);
}

#[test]
fn test_tuple_with_i128() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    // (I8, I128) — I128 has 16-byte alignment
    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I128),
    ]);

    // Fields start after count header (8 bytes)
    let (offset0, _) = field_layout(&tuple_kind, 0, &type_defs, ptr);
    assert_eq!(offset0, 8);

    // I128 alignment is 16 → offset 9 padded to 16
    let (offset1, ty1) = field_layout(&tuple_kind, 1, &type_defs, ptr);
    assert_eq!(offset1, 16);
    assert_eq!(ty1, types::I128);

    // Total = count@0 (8) + I8@8 (1) + I128@16 (16) = 32, max_align=16 → 32
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 32);
}

#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "tuple field index 2 out of range")
)]
fn test_tuple_out_of_range_field_idx() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
    ]);

    // Field index 2 is out of range — panics in debug, falls through in release
    let _ = field_layout(&tuple_kind, 2, &type_defs, ptr);
}

// ═══════════════════════════════════════════════════════════════════════
// Struct layout
// ═══════════════════════════════════════════════════════════════════════

fn make_struct(fields: Vec<(&str, TypeKind)>) -> StructDefinition {
    StructDefinition {
        fields: fields
            .into_iter()
            .map(|(name, kind)| {
                (
                    name.to_string(),
                    t(kind),
                    miri::ast::MemberVisibility::Public,
                )
            })
            .collect(),
        generics: None,
        module: String::new(),
    }
}

#[test]
fn test_struct_field_layout_with_padding() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![
        ("a", TypeKind::I16),
        ("b", TypeKind::F64),
        ("c", TypeKind::I64),
    ]);
    type_defs.insert("MyStruct".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("MyStruct".to_string(), None);

    // Field 0: I16 at offset 0
    let (offset, ty) = field_layout(&custom_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, types::I16);

    // Field 1: F64, align=8, offset 2 → 8
    let (offset, ty) = field_layout(&custom_kind, 1, &type_defs, ptr);
    assert_eq!(offset, 8);
    assert_eq!(ty, types::F64);

    // Field 2: I64, align=8, offset 16 → 16
    let (offset, ty) = field_layout(&custom_kind, 2, &type_defs, ptr);
    assert_eq!(offset, 16);
    assert_eq!(ty, types::I64);
}

#[test]
fn test_struct_aggregate_size() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![
        ("a", TypeKind::I16),
        ("b", TypeKind::F64),
        ("c", TypeKind::I64),
    ]);
    type_defs.insert("MyStruct".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("MyStruct".to_string(), None);
    // I16@0, F64@8, I64@16 → total=24, max_align=8 → 24
    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 24);
}

#[test]
fn test_struct_empty() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![]);
    type_defs.insert("Empty".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("Empty".to_string(), None);
    // Empty struct: total=0, max_align=ptr_size(8) → align_to(0, 8) = 0
    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 0);
}

#[test]
fn test_struct_single_field() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![("x", TypeKind::I32)]);
    type_defs.insert("Single".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("Single".to_string(), None);

    let (offset, ty) = field_layout(&custom_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, types::I32);

    // Size: 4, max_align=max(8,4)=8, align_to(4,8) = 8
    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 8);
}

#[test]
fn test_struct_uniform_fields_no_padding() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![
        ("a", TypeKind::I64),
        ("b", TypeKind::I64),
        ("c", TypeKind::I64),
    ]);
    type_defs.insert("Uniform".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("Uniform".to_string(), None);

    let (o0, _) = field_layout(&custom_kind, 0, &type_defs, ptr);
    let (o1, _) = field_layout(&custom_kind, 1, &type_defs, ptr);
    let (o2, _) = field_layout(&custom_kind, 2, &type_defs, ptr);
    assert_eq!(o0, 0);
    assert_eq!(o1, 8);
    assert_eq!(o2, 16);

    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 24);
}

#[test]
fn test_struct_worst_case_padding() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    // (I8, I64, I8, I64) — maximizes padding waste
    let struct_def = make_struct(vec![
        ("a", TypeKind::I8),
        ("b", TypeKind::I64),
        ("c", TypeKind::I8),
        ("d", TypeKind::I64),
    ]);
    type_defs.insert("Padded".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("Padded".to_string(), None);

    // I8@0, I64@8, I8@16, I64@24
    let (o0, _) = field_layout(&custom_kind, 0, &type_defs, ptr);
    let (o1, _) = field_layout(&custom_kind, 1, &type_defs, ptr);
    let (o2, _) = field_layout(&custom_kind, 2, &type_defs, ptr);
    let (o3, _) = field_layout(&custom_kind, 3, &type_defs, ptr);
    assert_eq!(o0, 0);
    assert_eq!(o1, 8);
    assert_eq!(o2, 16);
    assert_eq!(o3, 24);

    // Total = 24 + 8 = 32, max_align = 8 → 32
    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 32);
}

#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "struct 'S' field index 1 out of range")
)]
fn test_struct_out_of_range_field_idx() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![("x", TypeKind::I32)]);
    type_defs.insert("S".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("S".to_string(), None);

    // Field index 1 is out of range — panics in debug, falls through in release
    let _ = field_layout(&custom_kind, 1, &type_defs, ptr);
}

// ═══════════════════════════════════════════════════════════════════════
// Enum layout
// ═══════════════════════════════════════════════════════════════════════

fn make_enum(variants: Vec<(&str, Vec<TypeKind>)>) -> EnumDefinition {
    let mut map = BTreeMap::new();
    for (name, types) in variants {
        map.insert(name.to_string(), types.into_iter().map(t).collect());
    }
    EnumDefinition {
        variants: map,
        generics: None,
        module: String::new(),
    }
}

#[test]
fn test_enum_with_payload() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![
        ("None", vec![]),
        ("Some", vec![TypeKind::I32, TypeKind::I32]),
    ]);
    type_defs.insert("MyEnum".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("MyEnum".to_string(), None);

    // Discriminant + 2 payload fields = 3 * ptr_size
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), 3 * ptr.bytes());

    // Field 0: discriminant at offset 0
    assert_eq!(field_layout(&enum_kind, 0, &type_defs, ptr).0, 0);

    // Field 1: first payload at offset ptr_size
    assert_eq!(
        field_layout(&enum_kind, 1, &type_defs, ptr).0,
        ptr.bytes() as i32
    );

    // Field 2: second payload at offset 2 * ptr_size
    assert_eq!(
        field_layout(&enum_kind, 2, &type_defs, ptr).0,
        2 * (ptr.bytes() as i32)
    );
}

#[test]
fn test_enum_all_unit_variants() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![("A", vec![]), ("B", vec![]), ("C", vec![])]);
    type_defs.insert("Color".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("Color".to_string(), None);

    // All unit variants: discriminant only, no payload → ptr_size + 0
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), ptr.bytes());

    // Discriminant at offset 0
    let (offset, ty) = field_layout(&enum_kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, ptr);
}

#[test]
fn test_enum_single_variant() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![("Only", vec![TypeKind::I64])]);
    type_defs.insert("Single".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("Single".to_string(), None);

    // Discriminant + 1 field = 2 * ptr_size
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), 2 * ptr.bytes());
}

#[test]
fn test_enum_max_payload_wins() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![
        ("Small", vec![TypeKind::I32]),
        ("Big", vec![TypeKind::I32, TypeKind::I64, TypeKind::I32]),
    ]);
    type_defs.insert("Mixed".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("Mixed".to_string(), None);

    // Max payload is Big with 3 fields → 3 * ptr_size
    // Total = ptr_size (discriminant) + 3 * ptr_size = 4 * ptr_size
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), 4 * ptr.bytes());
}

#[test]
fn test_enum_discriminant_is_pointer_type() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![("A", vec![])]);
    type_defs.insert("E".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("E".to_string(), None);

    let (_, disc_ty) = field_layout(&enum_kind, 0, &type_defs, ptr);
    assert_eq!(disc_ty, ptr, "Discriminant should be pointer-typed");
}

#[test]
fn test_enum_payload_fields_are_pointer_typed() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![("Val", vec![TypeKind::I8, TypeKind::Boolean])]);
    type_defs.insert("E".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("E".to_string(), None);

    // Even though the actual types are I8 and Boolean, enum payload uses ptr-sized slots
    let (_, ty1) = field_layout(&enum_kind, 1, &type_defs, ptr);
    let (_, ty2) = field_layout(&enum_kind, 2, &type_defs, ptr);
    assert_eq!(ty1, ptr);
    assert_eq!(ty2, ptr);
}

// ═══════════════════════════════════════════════════════════════════════
// Class and Trait layout
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_class_is_pointer_sized() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    type_defs.insert(
        "MyClass".to_string(),
        TypeDefinition::Class(ClassDefinition {
            name: "MyClass".to_string(),
            generics: None,
            base_class: None,
            traits: vec![],
            fields: Vec::new(),
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
        }),
    );

    let kind = TypeKind::Custom("MyClass".to_string(), None);
    assert_eq!(aggregate_size(&kind, &type_defs, ptr), ptr.bytes());
}

#[test]
fn test_class_field_layout_uses_pointer_slots() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    type_defs.insert(
        "C".to_string(),
        TypeDefinition::Class(ClassDefinition {
            name: "C".to_string(),
            generics: None,
            base_class: None,
            traits: vec![],
            fields: vec![
                (
                    "f0".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                    },
                ),
                (
                    "f1".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                    },
                ),
                (
                    "f2".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                    },
                ),
            ],
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
        }),
    );

    let kind = TypeKind::Custom("C".to_string(), None);

    let (o0, t0) = field_layout(&kind, 0, &type_defs, ptr);
    let (o1, t1) = field_layout(&kind, 1, &type_defs, ptr);
    let (o2, t2) = field_layout(&kind, 2, &type_defs, ptr);
    // Offsets are relative to payload pointer (past the 16-byte header)
    assert_eq!(o0, 0);
    assert_eq!(o1, ptr.bytes() as i32);
    assert_eq!(o2, 2 * ptr.bytes() as i32);

    assert_eq!(t0, ptr);
    assert_eq!(t1, ptr);
    assert_eq!(t2, ptr);
}

#[test]
fn test_trait_is_pointer_sized() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    type_defs.insert(
        "MyTrait".to_string(),
        TypeDefinition::Trait(TraitDefinition {
            name: "MyTrait".to_string(),
            generics: None,
            parent_traits: vec![],
            methods: BTreeMap::new(),
            module: String::new(),
        }),
    );

    let kind = TypeKind::Custom("MyTrait".to_string(), None);
    assert_eq!(aggregate_size(&kind, &type_defs, ptr), ptr.bytes());
}

// ═══════════════════════════════════════════════════════════════════════
// Generic and Alias TypeDefinition
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_generic_typedef_is_pointer_sized() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    type_defs.insert(
        "T".to_string(),
        TypeDefinition::Generic(GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: miri::ast::types::TypeDeclarationKind::None,
        }),
    );

    let kind = TypeKind::Custom("T".to_string(), None);
    assert_eq!(aggregate_size(&kind, &type_defs, ptr), ptr.bytes());

    let (offset, ty) = field_layout(&kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, ptr);
}

#[test]
fn test_alias_to_primitive_resolves() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    // Alias to a primitive type — should resolve through to the primitive's layout
    type_defs.insert(
        "MyAlias".to_string(),
        TypeDefinition::Alias(AliasDefinition {
            template: t(TypeKind::I32),
            generics: None,
        }),
    );

    let kind = TypeKind::Custom("MyAlias".to_string(), None);
    // I32 is not an aggregate, so aggregate_size falls through to the wildcard → ptr_size
    assert_eq!(aggregate_size(&kind, &type_defs, ptr), ptr.bytes());
}

#[test]
fn test_alias_to_struct_resolves_layout() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    // Register a struct
    let struct_def = make_struct(vec![("x", TypeKind::I64), ("y", TypeKind::I64)]);
    type_defs.insert("Point".to_string(), TypeDefinition::Struct(struct_def));

    // Register an alias that resolves to the struct
    type_defs.insert(
        "Pos".to_string(),
        TypeDefinition::Alias(AliasDefinition {
            template: t(TypeKind::Custom("Point".to_string(), None)),
            generics: None,
        }),
    );

    let alias_kind = TypeKind::Custom("Pos".to_string(), None);
    let direct_kind = TypeKind::Custom("Point".to_string(), None);

    // Alias should resolve to the same layout as the struct
    assert_eq!(
        aggregate_size(&alias_kind, &type_defs, ptr),
        aggregate_size(&direct_kind, &type_defs, ptr)
    );

    // Field layout should match too
    let (alias_o0, alias_t0) = field_layout(&alias_kind, 0, &type_defs, ptr);
    let (direct_o0, direct_t0) = field_layout(&direct_kind, 0, &type_defs, ptr);
    assert_eq!(alias_o0, direct_o0);
    assert_eq!(alias_t0, direct_t0);

    let (alias_o1, alias_t1) = field_layout(&alias_kind, 1, &type_defs, ptr);
    let (direct_o1, direct_t1) = field_layout(&direct_kind, 1, &type_defs, ptr);
    assert_eq!(alias_o1, direct_o1);
    assert_eq!(alias_t1, direct_t1);
}

#[test]
fn test_alias_to_tuple_resolves_layout() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    // Alias that resolves to a tuple type
    let tuple_type = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I32),
        create_type_expr(TypeKind::I64),
    ]);
    type_defs.insert(
        "Pair".to_string(),
        TypeDefinition::Alias(AliasDefinition {
            template: t(tuple_type.clone()),
            generics: None,
        }),
    );

    let alias_kind = TypeKind::Custom("Pair".to_string(), None);

    // Should resolve through to tuple layout
    assert_eq!(
        aggregate_size(&alias_kind, &type_defs, ptr),
        aggregate_size(&tuple_type, &type_defs, ptr)
    );

    let (alias_o0, alias_t0) = field_layout(&alias_kind, 0, &type_defs, ptr);
    let (direct_o0, direct_t0) = field_layout(&tuple_type, 0, &type_defs, ptr);
    assert_eq!(alias_o0, direct_o0);
    assert_eq!(alias_t0, direct_t0);
}

// ═══════════════════════════════════════════════════════════════════════
// Missing type definition (fallback behavior)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_unknown_custom_type_fallback() {
    let type_defs = HashMap::new(); // empty — type not registered
    let ptr = ptr_ty();

    let kind = TypeKind::Custom("NonExistent".to_string(), None);

    // Falls back to pointer-sized
    assert_eq!(aggregate_size(&kind, &type_defs, ptr), ptr.bytes());

    let (offset, ty) = field_layout(&kind, 0, &type_defs, ptr);
    assert_eq!(offset, 0);
    assert_eq!(ty, ptr);

    let (offset1, _) = field_layout(&kind, 1, &type_defs, ptr);
    assert_eq!(offset1, ptr.bytes() as i32);
}

// ═══════════════════════════════════════════════════════════════════════
// Non-aggregate types (wildcard arm in field_layout)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_non_aggregate_types_use_pointer_slots() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    // Primitive types hitting the wildcard arm
    let (o0, t0) = field_layout(&TypeKind::I32, 0, &type_defs, ptr);
    assert_eq!(o0, 0);
    assert_eq!(t0, ptr);

    let (o1, t1) = field_layout(&TypeKind::I32, 1, &type_defs, ptr);
    assert_eq!(o1, ptr.bytes() as i32);
    assert_eq!(t1, ptr);

    // String type
    let (o0, _) = field_layout(&TypeKind::String, 0, &type_defs, ptr);
    assert_eq!(o0, 0);

    // Collection types also hit the wildcard
    let list_kind = TypeKind::List(Box::new(create_type_expr(TypeKind::I32)));
    assert_eq!(aggregate_size(&list_kind, &type_defs, ptr), ptr.bytes());
}

// ═══════════════════════════════════════════════════════════════════════
// 32-bit pointer width
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_layout_with_32bit_pointers() {
    let type_defs = HashMap::new();
    let ptr32 = types::I32;

    // Tuple (I8, I32) with 32-bit pointers
    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
    ]);

    // Fields start after count header (ptr_size = 4 bytes for 32-bit)
    let (o0, _) = field_layout(&tuple_kind, 0, &type_defs, ptr32);
    let (o1, _) = field_layout(&tuple_kind, 1, &type_defs, ptr32);
    assert_eq!(o0, 4);
    assert_eq!(o1, 8); // I32 alignment = 4, offset 5 → 8

    // Size: count@0 (4) + I8@4 (1) + I32@8 (4) = 12, max_align=max(4,1,4)=4, align_to(12,4)=12
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr32), 12);
}

#[test]
fn test_enum_layout_with_32bit_pointers() {
    let mut type_defs = HashMap::new();
    let ptr32 = types::I32;

    let enum_def = make_enum(vec![("None", vec![]), ("Some", vec![TypeKind::I32])]);
    type_defs.insert("Opt".to_string(), TypeDefinition::Enum(enum_def));

    let kind = TypeKind::Custom("Opt".to_string(), None);

    // 32-bit: discriminant (4) + 1 field (4) = 8
    assert_eq!(aggregate_size(&kind, &type_defs, ptr32), 8);

    let (disc_off, _) = field_layout(&kind, 0, &type_defs, ptr32);
    let (payload_off, _) = field_layout(&kind, 1, &type_defs, ptr32);
    assert_eq!(disc_off, 0);
    assert_eq!(payload_off, 4);
}
