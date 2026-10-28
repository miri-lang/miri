// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use cranelift_codegen::ir::types;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::layout::{
    aggregate_layout, aggregate_size, class_payload_layout, enum_payload_layout, field_layout,
    EnumColumn, EnumLayout, Member,
};
use miri::codegen::cranelift::value_locals::scalar_type;
use miri::mir::type_facts::{held_value_layout, FieldRepr};

use miri::error::syntax::Span;
use miri::error::CodegenError;
use miri::mir::option_layout::LayoutScope;
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

    // A tuple of scalars is held as its bytes, with no count header.
    // Field 0: I8 at offset 0
    let (offset, ty) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(types::I8));

    // Field 1: I32, alignment 4 → offset 1 padded to 4
    let (offset, ty) = field_layout(&tuple_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 4);
    assert_eq!(ty, Member::Word(types::I32));

    // Field 2: I64, alignment 8 → offset 4+4=8, already aligned
    let (offset, ty) = field_layout(&tuple_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 8);
    assert_eq!(ty, Member::Word(types::I64));
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

    // Allocated as a tuple, the count word comes first:
    // count@0, I8@8, I32@12, I64@16 → end=24, max_align=8 → final=24
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 24);
}

#[test]
fn test_tuple_single_field() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![create_type_expr(TypeKind::I32)]);

    // A tuple of scalars is held as its bytes: field 0 starts at its start.
    let (offset, ty) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(types::I32));

    // Allocated as a tuple: count@0 (8 bytes) + I32@8 (4 bytes) = 12,
    // max_align=8 → align_to(12, 8) = 16
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

    // A tuple of scalars is held as its bytes, with no count header.
    let (offset0, _) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (offset1, _) = field_layout(&tuple_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset0, 0);
    assert_eq!(offset1, 4);

    // Allocated as a tuple: count@0 (8) + I32@8 (4) + I32@12 (4) = 16, max_align=8 → 16
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
    let (offset0, ty0) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset0, 8);
    assert_eq!(ty0, Member::Word(ptr));

    let (offset1, ty1) = field_layout(&tuple_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset1, 16);
    assert_eq!(ty1, Member::Word(types::I32));

    let (offset2, ty2) = field_layout(&tuple_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset2, 24);
    assert_eq!(ty2, Member::Word(ptr));

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

    // A tuple of scalars is held as its bytes, with no count header.
    let (offset0, _) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset0, 0);

    // I128 alignment is 16 → offset 1 padded to 16
    let (offset1, ty1) = field_layout(&tuple_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset1, 16);
    assert_eq!(ty1, Member::Word(types::I128));

    // Allocated as a tuple: count@0 (8) + I8@8 (1) + I128@16 (16) = 32,
    // max_align=16 → 32
    assert_eq!(aggregate_size(&tuple_kind, &type_defs, ptr), 32);
}

#[test]
fn test_tuple_out_of_range_field_idx() {
    let type_defs = HashMap::new();
    let ptr = ptr_ty();

    let tuple_kind = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I32),
    ]);

    // Field index 2 names no element, so no offset answers it.
    let error = field_layout(&tuple_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect_err("an index past the last element has no layout");
    assert!(
        error.to_string().contains("field index 2 out of range"),
        "unexpected error: {error}"
    );
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
    let (offset, ty) = field_layout(&custom_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(types::I16));

    // Field 1: F64, align=8, offset 2 → 8
    let (offset, ty) = field_layout(&custom_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 8);
    assert_eq!(ty, Member::Word(types::F64));

    // Field 2: I64, align=8, offset 16 → 16
    let (offset, ty) = field_layout(&custom_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 16);
    assert_eq!(ty, Member::Word(types::I64));
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

    let (offset, ty) = field_layout(&custom_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(types::I32));

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

    let (o0, _) = field_layout(&custom_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o1, _) = field_layout(&custom_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o2, _) = field_layout(&custom_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
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
    let (o0, _) = field_layout(&custom_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o1, _) = field_layout(&custom_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o2, _) = field_layout(&custom_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o3, _) = field_layout(&custom_kind, 3, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(o0, 0);
    assert_eq!(o1, 8);
    assert_eq!(o2, 16);
    assert_eq!(o3, 24);

    // Total = 24 + 8 = 32, max_align = 8 → 32
    assert_eq!(aggregate_size(&custom_kind, &type_defs, ptr), 32);
}

#[test]
fn test_struct_out_of_range_field_idx() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let struct_def = make_struct(vec![("x", TypeKind::I32)]);
    type_defs.insert("S".to_string(), TypeDefinition::Struct(struct_def));

    let custom_kind = TypeKind::Custom("S".to_string(), None);

    // Field index 1 names no field, so no offset answers it.
    let error = field_layout(&custom_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect_err("an index past the last field has no layout");
    assert!(
        error.to_string().contains("field index 1 out of range"),
        "unexpected error: {error}"
    );
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
        methods: BTreeMap::new(),
        module: String::new(),
        must_use: false,
        non_exhaustive: false,
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
    assert_eq!(
        field_layout(&enum_kind, 0, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout")
            .0,
        0
    );

    // Field 1: first payload at offset ptr_size
    assert_eq!(
        field_layout(&enum_kind, 1, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout")
            .0,
        ptr.bytes() as i32
    );

    // Field 2: second payload at offset 2 * ptr_size
    assert_eq!(
        field_layout(&enum_kind, 2, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout")
            .0,
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
    let (offset, ty) = field_layout(&enum_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(ptr));
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

/// Field `idx` of `kind`'s offset, laid out against `defs`.
fn enum_field_offset(kind: &TypeKind, idx: usize, defs: &HashMap<String, TypeDefinition>) -> i32 {
    field_layout(kind, idx, LayoutScope::program(defs), ptr_ty())
        .expect("the field has a layout")
        .0
}

/// A 128-bit payload widens only the position it sits at: every variant's
/// field at that position shares the wide column, and the positions after it
/// keep their own width, so a field's offset still depends only on its index.
#[test]
fn test_enum_wide_payload_widens_only_its_column() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![
        ("Big", vec![TypeKind::I128, TypeKind::Int]),
        ("Small", vec![TypeKind::Int, TypeKind::Int, TypeKind::Int]),
    ]);
    type_defs.insert("Wide".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("Wide".to_string(), None);

    assert_eq!(enum_field_offset(&enum_kind, 0, &type_defs), 0);
    // The first payload column is aligned for the 128-bit value it may hold.
    assert_eq!(enum_field_offset(&enum_kind, 1, &type_defs), 16);
    assert_eq!(enum_field_offset(&enum_kind, 2, &type_defs), 32);
    assert_eq!(enum_field_offset(&enum_kind, 3, &type_defs), 40);
    // Padded to the widest column alignment.
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), 48);
}

/// An enum with `A(int)`, `B(<a 128-byte struct held as its bytes>)` and
/// `C(int, int)`. The held struct widens the first payload column alone: the
/// discriminant keeps one word and the second payload column keeps one word.
fn mixed_column_enum() -> HashMap<String, TypeDefinition> {
    let wide_fields = (0..16).map(|i| format!("f{i}")).collect::<Vec<_>>();
    let wide = make_struct(
        wide_fields
            .iter()
            .map(|name| (name.as_str(), TypeKind::Int))
            .collect(),
    );
    let enum_def = make_enum(vec![
        ("A", vec![TypeKind::Int]),
        ("B", vec![TypeKind::Custom("Wide".to_string(), None)]),
        ("C", vec![TypeKind::Int, TypeKind::Int]),
    ]);
    HashMap::from([
        ("Wide".to_string(), TypeDefinition::Struct(wide)),
        ("Columns".to_string(), TypeDefinition::Enum(enum_def)),
    ])
}

#[test]
fn test_enum_payload_columns_are_sized_per_position() {
    let type_defs = mixed_column_enum();
    let ptr = ptr_ty();
    let wide = TypeKind::Custom("Wide".to_string(), None);
    assert_eq!(
        held_value_layout(&wide, LayoutScope::program(&type_defs)).map(|layout| layout.size),
        Some(128),
        "the payload struct is held as its 128 bytes"
    );

    let enum_kind = TypeKind::Custom("Columns".to_string(), None);
    // The discriminant takes one word.
    assert_eq!(enum_field_offset(&enum_kind, 0, &type_defs), 0);
    // Payload position 0 is as wide as the held struct.
    assert_eq!(enum_field_offset(&enum_kind, 1, &type_defs), 8);
    // Payload position 1 starts right after it, one word wide.
    assert_eq!(enum_field_offset(&enum_kind, 2, &type_defs), 136);
    // One word of discriminant, 128 bytes, one word: 144 bytes. Sizing every
    // slot at the widest payload took 128 * (1 + 2) = 384.
    assert_eq!(aggregate_size(&enum_kind, &type_defs, ptr), 144);
}

#[test]
fn test_enum_payload_layout_names_each_column() {
    let type_defs = mixed_column_enum();
    let Some(TypeDefinition::Enum(enum_def)) = type_defs.get("Columns") else {
        panic!("the enum is defined");
    };
    let layout = enum_payload_layout(enum_def, None, LayoutScope::program(&type_defs), ptr_ty());
    let columns: Vec<(i32, i32)> = layout
        .columns
        .iter()
        .map(|column| (column.offset, column.size))
        .collect();
    assert_eq!(columns, vec![(0, 8), (8, 128), (136, 8)]);
    assert_eq!(layout.size, 144);
    // A value of `A` carries the discriminant and one payload field.
    assert_eq!(layout.value_size(2), Some(136));
    assert_eq!(layout.value_size(3), Some(144));
    assert_eq!(layout.value_size(4), None);
}

#[test]
fn test_enum_field_past_the_last_column_is_an_internal_error() {
    let type_defs = mixed_column_enum();
    let enum_kind = TypeKind::Custom("Columns".to_string(), None);

    // The enum lays out three columns: the discriminant and two payloads.
    let error = field_layout(&enum_kind, 3, LayoutScope::program(&type_defs), ptr_ty())
        .expect_err("an index past the last column has no layout");
    assert!(
        matches!(&error, CodegenError::Internal(message)
            if message.contains("field index 3 out of range")
                && message.contains("at most 3 fields")),
        "unexpected error: {error}"
    );
}

#[test]
fn test_enum_layout_words_gives_each_field_a_word() {
    let layout = EnumLayout::words(3, ptr_ty());
    assert_eq!(
        layout.columns,
        vec![
            EnumColumn { offset: 0, size: 8 },
            EnumColumn { offset: 8, size: 8 },
            EnumColumn {
                offset: 16,
                size: 8
            },
        ]
    );
    assert_eq!(layout.size, 24);
    assert_eq!(layout.value_size(3), Some(24));

    let empty = EnumLayout::words(0, ptr_ty());
    assert!(empty.columns.is_empty());
    assert_eq!(empty.size, 0);
}

#[test]
fn test_enum_value_of_no_fields_needs_no_bytes() {
    let type_defs = mixed_column_enum();
    let Some(TypeDefinition::Enum(enum_def)) = type_defs.get("Columns") else {
        panic!("the enum is defined");
    };
    let layout = enum_payload_layout(enum_def, None, LayoutScope::program(&type_defs), ptr_ty());
    assert_eq!(layout.value_size(0), Some(0));
    // A payload-less value carries only its discriminant: one word.
    assert_eq!(layout.value_size(1), Some(8));
}

#[test]
fn test_enum_discriminant_is_pointer_type() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    let enum_def = make_enum(vec![("A", vec![])]);
    type_defs.insert("E".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("E".to_string(), None);

    let (_, disc_ty) = field_layout(&enum_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(
        disc_ty,
        Member::Word(ptr),
        "Discriminant should be pointer-typed"
    );
}

#[test]
fn test_enum_payload_fields_are_pointer_typed() {
    let mut type_defs = HashMap::new();
    let ptr = ptr_ty();

    // The string payload keeps the enum a counted allocation.
    let enum_def = make_enum(vec![
        ("Val", vec![TypeKind::I8, TypeKind::Boolean]),
        ("Text", vec![TypeKind::String]),
    ]);
    type_defs.insert("E".to_string(), TypeDefinition::Enum(enum_def));

    let enum_kind = TypeKind::Custom("E".to_string(), None);

    // Even though the actual types are I8 and Boolean, a counted enum's
    // payload uses ptr-sized slots
    let (_, ty1) = field_layout(&enum_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (_, ty2) = field_layout(&enum_kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(ty1, Member::Word(ptr));
    assert_eq!(ty2, Member::Word(ptr));
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
            base_class_args: None,
            traits: vec![],
            trait_args: std::collections::HashMap::new(),
            fields: Vec::new(),
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
            runtime_settled_methods: std::collections::BTreeSet::new(),
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
            base_class_args: None,
            traits: vec![],
            trait_args: std::collections::HashMap::new(),
            fields: vec![
                (
                    "f0".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                        initializer: None,
                    },
                ),
                (
                    "f1".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                        initializer: None,
                    },
                ),
                (
                    "f2".to_string(),
                    FieldInfo {
                        ty: t(TypeKind::I64),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                        initializer: None,
                    },
                ),
            ],
            methods: BTreeMap::new(),
            module: String::new(),
            is_abstract: false,
            runtime_settled_methods: std::collections::BTreeSet::new(),
        }),
    );

    let kind = TypeKind::Custom("C".to_string(), None);

    let (o0, t0) = field_layout(&kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o1, t1) = field_layout(&kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (o2, t2) = field_layout(&kind, 2, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    // Offsets are relative to the payload pointer (past the 16-byte header);
    // every class keeps its first word for the vtable pointer.
    assert_eq!(o0, ptr.bytes() as i32);
    assert_eq!(o1, 2 * ptr.bytes() as i32);
    assert_eq!(o2, 3 * ptr.bytes() as i32);

    assert_eq!(t0, Member::Word(ptr));
    assert_eq!(t1, Member::Word(ptr));
    assert_eq!(t2, Member::Word(ptr));
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
            parent_trait_args: BTreeMap::new(),
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

    let (offset, ty) = field_layout(&kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(ptr));
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
    let (alias_o0, alias_t0) = field_layout(&alias_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (direct_o0, direct_t0) =
        field_layout(&direct_kind, 0, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout");
    assert_eq!(alias_o0, direct_o0);
    assert_eq!(alias_t0, direct_t0);

    let (alias_o1, alias_t1) = field_layout(&alias_kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (direct_o1, direct_t1) =
        field_layout(&direct_kind, 1, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout");
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

    let (alias_o0, alias_t0) = field_layout(&alias_kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    let (direct_o0, direct_t0) =
        field_layout(&tuple_type, 0, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout");
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

    let (offset, ty) = field_layout(&kind, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(offset, 0);
    assert_eq!(ty, Member::Word(ptr));

    let (offset1, _) = field_layout(&kind, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
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
    let (o0, t0) = field_layout(&TypeKind::I32, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(o0, 0);
    assert_eq!(t0, Member::Word(ptr));

    let (o1, t1) = field_layout(&TypeKind::I32, 1, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
    assert_eq!(o1, ptr.bytes() as i32);
    assert_eq!(t1, Member::Word(ptr));

    // String type
    let (o0, _) = field_layout(&TypeKind::String, 0, LayoutScope::program(&type_defs), ptr)
        .expect("the field has a layout");
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

    // A tuple of scalars is held as its bytes, whatever the pointer width.
    let (o0, _) = field_layout(&tuple_kind, 0, LayoutScope::program(&type_defs), ptr32)
        .expect("the field has a layout");
    let (o1, _) = field_layout(&tuple_kind, 1, LayoutScope::program(&type_defs), ptr32)
        .expect("the field has a layout");
    assert_eq!(o0, 0);
    assert_eq!(o1, 4); // I32 alignment = 4, offset 1 → 4

    // Allocated as a tuple: count@0 (4) + I8@4 (1) + I32@8 (4) = 12,
    // max_align=max(4,1,4)=4, align_to(12,4)=12
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

    // The columns a counted enum is laid out in follow the pointer width. An
    // enum held as its bytes is laid out by the value-class authority, whose
    // word is the host's, so the columns are read here directly.
    let Some(TypeDefinition::Enum(enum_def)) = type_defs.get("Opt") else {
        panic!("the enum is defined");
    };
    let layout = enum_payload_layout(enum_def, None, LayoutScope::program(&type_defs), ptr32);
    let offsets: Vec<i32> = layout.columns.iter().map(|column| column.offset).collect();
    assert_eq!(offsets, vec![0, 4]);
}

/// A class definition carrying `fields`, extending `base` when one is named.
fn make_class(name: &str, base: Option<&str>, fields: Vec<(&str, TypeKind)>) -> ClassDefinition {
    ClassDefinition {
        name: name.to_string(),
        generics: None,
        base_class: base.map(str::to_string),
        base_class_args: None,
        traits: vec![],
        trait_args: std::collections::HashMap::new(),
        fields: fields
            .into_iter()
            .map(|(field_name, kind)| {
                (
                    field_name.to_string(),
                    FieldInfo {
                        ty: t(kind),
                        mutable: false,
                        visibility: MemberVisibility::Public,
                        initializer: None,
                    },
                )
            })
            .collect(),
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: std::collections::BTreeSet::new(),
    }
}

#[test]
fn class_payload_size_covers_every_field_including_inherited_ones() {
    // The payload an instance is allocated and the offsets its fields are
    // reached at come from one walk, so that an instance cannot be given less
    // memory than the fields written into it. A child that counts only what it
    // declares itself would size this one at 24 bytes and leave the base's two
    // fields outside the allocation.
    let ptr = ptr_ty();
    let mut type_defs = HashMap::new();
    type_defs.insert(
        "Base".to_string(),
        TypeDefinition::Class(make_class(
            "Base",
            None,
            vec![("a", TypeKind::Float), ("b", TypeKind::Float)],
        )),
    );
    type_defs.insert(
        "Child".to_string(),
        TypeDefinition::Class(make_class(
            "Child",
            Some("Base"),
            vec![("c", TypeKind::Float), ("d", TypeKind::Float)],
        )),
    );

    let layout = class_payload_layout("Child", None, LayoutScope::program(&type_defs), ptr);

    assert_eq!(
        layout.fields.len(),
        4,
        "an instance holds the base's fields as well as its own"
    );
    assert_eq!(
        layout.size, 40,
        "four eight-byte fields after the vtable word need forty bytes"
    );

    let kind = TypeKind::Custom("Child".to_string(), None);
    for index in 0..layout.fields.len() {
        let (offset, member) = field_layout(&kind, index, LayoutScope::program(&type_defs), ptr)
            .expect("the field has a layout");
        let cl_ty = member.word().expect("a scalar class field is a word");
        assert_eq!(
            (offset, Member::Word(cl_ty)),
            layout.fields[index],
            "field {} is reached at a different place than it is laid out",
            index
        );
        assert!(
            offset + cl_ty.bytes() as i32 <= layout.size as i32,
            "field {} ends past the {} bytes the payload is given",
            index,
            layout.size
        );
    }
}

#[test]
fn a_base_field_sits_where_it_does_in_a_subclass_that_dispatches() {
    // A base without a vtable and a subclass that dispatches share the base's
    // bodies, so the base's fields sit at the same offsets in both.
    let ptr = ptr_ty();
    let mut type_defs = HashMap::new();
    type_defs.insert(
        "Base".to_string(),
        TypeDefinition::Class(make_class("Base", None, vec![("id", TypeKind::Int)])),
    );
    let mut child = make_class("Child", Some("Base"), vec![("extra", TypeKind::Int)]);
    child.traits = vec!["Named".to_string()];
    type_defs.insert("Child".to_string(), TypeDefinition::Class(child));

    let base = class_payload_layout("Base", None, LayoutScope::program(&type_defs), ptr);
    let child = class_payload_layout("Child", None, LayoutScope::program(&type_defs), ptr);

    assert_eq!(base.fields[0], child.fields[0]);
    assert_eq!(base.fields[0].0, ptr.bytes() as i32);
}

#[test]
fn test_optional_field_layout_refuses_an_optional_with_no_box() {
    // An optional of a class is the class pointer itself; an offset into it
    // would read the class's own first word as the payload.
    let node = TypeKind::Custom("Node".to_string(), None);
    let defs = HashMap::from([(
        "Node".to_string(),
        TypeDefinition::Class(make_class("Node", None, vec![("v", TypeKind::Int)])),
    )]);
    let pointer = TypeKind::Option(Box::new(t(node)));
    let error = field_layout(&pointer, 0, LayoutScope::program(&defs), ptr_ty())
        .expect_err("a nullable-pointer optional has no payload offset");
    assert!(
        error.to_string().contains("no box to hold the payload"),
        "unexpected error: {error}"
    );

    let boxed = TypeKind::Option(Box::new(t(pointer.clone())));
    assert_eq!(
        field_layout(&boxed, 0, LayoutScope::program(&defs), ptr_ty())
            .expect("a boxed optional has a payload slot"),
        word(0, ptr_ty())
    );

    // An optional held as its bytes holds its payload beside its tag.
    let inline = TypeKind::Option(Box::new(t(TypeKind::Int)));
    assert_eq!(
        field_layout(&inline, 0, LayoutScope::program(&defs), ptr_ty())
            .expect("an inline optional has a payload beside its tag"),
        word(8, ptr_ty())
    );
}

#[test]
fn test_generic_alias_field_layout_is_its_template_at_its_arguments() {
    // `type Id<T> is T` at a tuple lays out as that tuple.
    let defs = HashMap::from([(
        "Id".to_string(),
        TypeDefinition::Alias(AliasDefinition {
            template: t(TypeKind::Custom("T".to_string(), None)),
            generics: Some(vec![GenericDefinition {
                name: "T".to_string(),
                constraint: None,
                kind: miri::ast::types::TypeDeclarationKind::None,
            }]),
        }),
    )]);
    let tuple = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::I64),
    ]);
    let aliased = TypeKind::Custom(
        "Id".to_string(),
        Some(vec![create_type_expr(tuple.clone())]),
    );
    assert_eq!(
        field_layout(&aliased, 1, LayoutScope::program(&defs), ptr_ty())
            .expect("the alias has a layout"),
        field_layout(&tuple, 1, LayoutScope::program(&defs), ptr_ty())
            .expect("the tuple has a layout"),
    );
}

/// The layout a struct or tuple of `kind` is built with: its own bytes for a
/// value held as its bytes, which is built in place member by member, and its
/// payload layout for one held as a reference.
fn built_layout(kind: &TypeKind, scope: LayoutScope<'_>) -> (Vec<(i32, Member)>, u32) {
    let Some(value) = held_value_layout(kind, scope) else {
        let layout = aggregate_layout(kind, scope, ptr_ty())
            .expect("a struct or tuple has a payload layout");
        return (layout.fields, layout.size);
    };
    let fields = value
        .fields
        .iter()
        .map(|field| {
            let member = match &field.repr {
                FieldRepr::Scalar(scalar) => Member::Word(scalar_type(*scalar)),
                FieldRepr::Value(inner) => Member::Inline(inner.clone()),
                FieldRepr::OptionTag => panic!("no struct or tuple field is an optional's tag"),
                FieldRepr::Overlay(_) => panic!("no struct or tuple field is an enum's column"),
            };
            (field.offset as i32, member)
        })
        .collect();
    (fields, value.size)
}

/// A word member of type `ty` at `offset`.
fn word(offset: i32, ty: types::Type) -> (i32, Member) {
    (offset, Member::Word(ty))
}

/// Asserts that the layout a struct or tuple of `kind` is built with places
/// every field exactly where `field_layout` reads it from, and returns it.
fn built_layout_agrees_with_reads(
    kind: &TypeKind,
    defs: &HashMap<String, TypeDefinition>,
) -> (Vec<(i32, Member)>, u32) {
    let scope = LayoutScope::program(defs);
    let layout = built_layout(kind, scope);
    for (index, placed) in layout.0.iter().enumerate() {
        let read = field_layout(kind, index, scope, ptr_ty()).expect("the field has a layout");
        assert_eq!(
            *placed, read,
            "field {index} of {kind:?} is built apart from where it is read"
        );
    }
    layout
}

fn mixed_widths() -> StructDefinition {
    make_struct(vec![
        ("a", TypeKind::I8),
        ("b", TypeKind::Int),
        ("c", TypeKind::F32),
    ])
}

#[test]
fn a_mixed_width_struct_is_built_where_its_fields_are_read() {
    let defs = HashMap::from([("Mixed".to_string(), TypeDefinition::Struct(mixed_widths()))]);
    let mixed = TypeKind::Custom("Mixed".to_string(), None);
    assert_eq!(
        built_layout_agrees_with_reads(&mixed, &defs),
        (
            vec![
                word(0, types::I8),
                word(8, types::I64),
                word(16, types::F32)
            ],
            24
        )
    );
}

#[test]
fn a_mixed_width_tuple_is_built_where_its_elements_are_read() {
    let defs = HashMap::new();
    let tuple = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::I8),
        create_type_expr(TypeKind::Int),
        create_type_expr(TypeKind::F32),
    ]);
    // A tuple of scalars is held as its bytes, with no count header.
    assert_eq!(
        built_layout_agrees_with_reads(&tuple, &defs),
        (
            vec![
                word(0, types::I8),
                word(8, types::I64),
                word(16, types::F32)
            ],
            24
        )
    );
}

#[test]
fn a_nested_struct_field_is_built_inline_where_it_is_read() {
    let outer = make_struct(vec![
        ("flag", TypeKind::Boolean),
        ("inner", TypeKind::Custom("Mixed".to_string(), None)),
        ("w", TypeKind::I16),
    ]);
    let defs = HashMap::from([
        ("Mixed".to_string(), TypeDefinition::Struct(mixed_widths())),
        ("Outer".to_string(), TypeDefinition::Struct(outer)),
    ]);
    let kind = TypeKind::Custom("Outer".to_string(), None);
    let mixed = TypeKind::Custom("Mixed".to_string(), None);
    let inner_layout = held_value_layout(&mixed, LayoutScope::program(&defs))
        .expect("a struct of scalars is held as its bytes");
    // `inner` sits inline, all 24 bytes of it, read as the address of them.
    assert_eq!(
        built_layout_agrees_with_reads(&kind, &defs),
        (
            vec![
                word(0, types::I8),
                (8, Member::Inline(inner_layout)),
                word(32, types::I16)
            ],
            40
        )
    );
}

#[test]
fn a_128_bit_field_is_built_at_its_own_alignment() {
    let wide = make_struct(vec![
        ("a", TypeKind::Boolean),
        ("w", TypeKind::I128),
        ("b", TypeKind::Boolean),
    ]);
    let defs = HashMap::from([("Wide".to_string(), TypeDefinition::Struct(wide))]);
    let kind = TypeKind::Custom("Wide".to_string(), None);
    assert_eq!(
        built_layout_agrees_with_reads(&kind, &defs),
        (
            vec![
                word(0, types::I8),
                word(16, types::I128),
                word(32, types::I8)
            ],
            48
        )
    );

    let tuple = TypeKind::Tuple(vec![
        create_type_expr(TypeKind::Boolean),
        create_type_expr(TypeKind::U128),
    ]);
    assert_eq!(
        built_layout_agrees_with_reads(&tuple, &HashMap::new()),
        (vec![word(0, types::I8), word(16, types::I128)], 32)
    );
}

#[test]
fn adjacent_bool_fields_are_built_byte_by_byte() {
    let flags = make_struct(vec![
        ("a", TypeKind::Boolean),
        ("b", TypeKind::Boolean),
        ("n", TypeKind::I32),
    ]);
    let defs = HashMap::from([("Flags".to_string(), TypeDefinition::Struct(flags))]);
    let kind = TypeKind::Custom("Flags".to_string(), None);
    assert_eq!(
        built_layout_agrees_with_reads(&kind, &defs),
        (
            vec![word(0, types::I8), word(1, types::I8), word(4, types::I32)],
            8
        )
    );
}

#[test]
fn a_vector_is_built_component_after_component() {
    let vector = TypeKind::Custom(
        "Vec3".to_string(),
        Some(vec![create_type_expr(TypeKind::F32)]),
    );
    assert_eq!(
        built_layout_agrees_with_reads(&vector, &HashMap::new()),
        (
            vec![
                word(0, types::F32),
                word(4, types::F32),
                word(8, types::F32)
            ],
            12
        )
    );
}

#[test]
fn only_a_struct_tuple_or_vector_has_an_aggregate_layout() {
    let defs = HashMap::from([(
        "C".to_string(),
        TypeDefinition::Class(make_class("C", None, vec![("x", TypeKind::Int)])),
    )]);
    let scope = LayoutScope::program(&defs);
    for kind in [
        TypeKind::Custom("C".to_string(), None),
        TypeKind::Custom("Unknown".to_string(), None),
        TypeKind::Int,
        TypeKind::Option(Box::new(t(TypeKind::Int))),
    ] {
        assert!(
            aggregate_layout(&kind, scope, ptr_ty()).is_none(),
            "{kind:?} is not laid out as a struct or tuple"
        );
    }
}

/// An enum held as its bytes places its tag and each payload position where
/// [`enum_payload_layout`] puts that column, so a value read in place and one
/// built into a counted holder agree on every offset.
#[test]
fn test_held_enum_offsets_are_the_enum_columns() {
    let pair = make_struct(vec![("small", TypeKind::I8), ("whole", TypeKind::I32)]);
    let enums = [
        make_enum(vec![("North", vec![]), ("South", vec![])]),
        make_enum(vec![
            ("Circle", vec![TypeKind::Float]),
            ("Rect", vec![TypeKind::Float, TypeKind::Float]),
        ]),
        make_enum(vec![
            ("Big", vec![TypeKind::I128, TypeKind::Int]),
            ("Small", vec![TypeKind::Int, TypeKind::Int, TypeKind::Int]),
        ]),
        make_enum(vec![
            ("Flag", vec![TypeKind::Boolean, TypeKind::F32]),
            ("Both", vec![TypeKind::Custom("Pair".to_string(), None)]),
            ("Byte", vec![TypeKind::U8, TypeKind::I16, TypeKind::Int]),
        ]),
    ];
    for enum_def in enums {
        let type_defs = HashMap::from([
            ("Pair".to_string(), TypeDefinition::Struct(pair.clone())),
            ("E".to_string(), TypeDefinition::Enum(enum_def.clone())),
        ]);
        let scope = LayoutScope::program(&type_defs);
        let kind = TypeKind::Custom("E".to_string(), None);
        let held = held_value_layout(&kind, scope).expect("the enum is held as its bytes");
        let columns = enum_payload_layout(&enum_def, None, scope, ptr_ty());
        let held_offsets: Vec<i32> = held.fields.iter().map(|f| f.offset as i32).collect();
        let column_offsets: Vec<i32> = columns.columns.iter().map(|c| c.offset).collect();
        assert_eq!(held_offsets, column_offsets, "offsets of {enum_def:?}");
        assert_eq!(held.size, columns.size, "size of {enum_def:?}");
    }
}

/// A held enum's payload column whose variants all store one member is read
/// as that member, a column of several scalars as a word a reader narrows to
/// its binding, and a column overlaying a held value with other members is
/// refused rather than read as a word.
#[test]
fn test_held_enum_columns_are_read_as_their_members() {
    let ptr = ptr_ty();
    let pair = make_struct(vec![("small", TypeKind::I8), ("whole", TypeKind::I32)]);
    let enum_def = make_enum(vec![
        (
            "Both",
            vec![TypeKind::I8, TypeKind::Custom("Pair".to_string(), None)],
        ),
        (
            "Byte",
            vec![TypeKind::Int, TypeKind::Int, TypeKind::Boolean],
        ),
        ("Flag", vec![TypeKind::Float]),
    ]);
    let type_defs = HashMap::from([
        ("Pair".to_string(), TypeDefinition::Struct(pair)),
        ("E".to_string(), TypeDefinition::Enum(enum_def)),
    ]);
    let scope = LayoutScope::program(&type_defs);
    let kind = TypeKind::Custom("E".to_string(), None);
    let member = |index| field_layout(&kind, index, scope, ptr).map(|(_, member)| member);
    assert!(matches!(member(1), Ok(Member::Word(ty)) if ty == ptr));
    assert!(member(2).is_err());
    assert!(matches!(member(3), Ok(Member::Word(ty)) if ty == types::I8));
}
