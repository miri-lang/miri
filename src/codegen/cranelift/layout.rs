// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Layout computation for aggregate types.
//!
//! Provides helpers to compute byte offsets and sizes for fields within
//! structs, tuples, and enums during Cranelift code generation.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::types::translate_type_kind;
use crate::error::CodegenError;
use crate::mir::instantiation::{field_types, member_type_at};
use crate::mir::option_layout::{is_nullable_pointer_optional, resolve_alias, LayoutScope};
use crate::mir::type_facts::{
    held_value_layout, EnumDefinition, FieldRepr, StructDefinition, TypeDefinition, ValueLayout,
};
use cranelift_codegen::ir::Type as CraneliftType;
use std::collections::HashMap;

/// Align an offset up to the given alignment.
fn align_to(offset: i32, alignment: i32) -> i32 {
    if alignment <= 1 {
        return offset;
    }
    (offset + alignment - 1) & !(alignment - 1)
}

/// What one member of a payload or of a value held as its bytes is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    /// A scalar or a word, read and written at this Cranelift type.
    Word(CraneliftType),
    /// A value held inline as its bytes, laid out as this. A read of it
    /// yields the address of those bytes; a write copies them; equality
    /// compares them member by member.
    Inline(ValueLayout),
}

impl Member {
    /// The Cranelift type a word member is read at, or `None` for a value
    /// held inline.
    pub fn word(&self) -> Option<CraneliftType> {
        match self {
            Member::Word(ty) => Some(*ty),
            Member::Inline(_) => None,
        }
    }
}

/// The room one member takes in a payload: what it is, and the bytes and
/// alignment it occupies.
#[derive(Debug, Clone)]
struct Slot {
    member: Member,
    size: i32,
    align: i32,
}

impl Slot {
    /// A scalar or a word, as wide and as aligned as its Cranelift type.
    fn of(ty: CraneliftType) -> Self {
        let bytes = ty.bytes() as i32;
        Self {
            member: Member::Word(ty),
            size: bytes,
            align: bytes,
        }
    }

    /// The slot a member of `kind` takes, read against `scope`: a value held
    /// as its bytes sits inline at its own size and alignment.
    fn member(kind: &TypeKind, scope: LayoutScope<'_>, ptr_ty: CraneliftType) -> Self {
        match held_value_layout(kind, scope) {
            Some(layout) => Self {
                size: layout.size as i32,
                align: layout.align as i32,
                member: Member::Inline(layout),
            },
            None => Self::of(translate_type_kind(kind, ptr_ty)),
        }
    }

    /// The slot of a tuple element written as `expr`. Only a type expression
    /// names one; anything else is pointer-sized.
    fn element(expr: &Expression, scope: LayoutScope<'_>, ptr_ty: CraneliftType) -> Self {
        match &expr.node {
            ExpressionKind::Type(ty, _) => Self::member(&ty.kind, scope, ptr_ty),
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
            | ExpressionKind::Block(..)
            | ExpressionKind::Cast(..) => Self::of(ptr_ty),
        }
    }
}

/// Where member `index` of a value held as its bytes, laid out as `layout`,
/// sits, and what it is: a scalar at its own width, or a nested value. An
/// optional's one member is its payload, beside its tag.
fn value_member(
    layout: &ValueLayout,
    index: usize,
    kind: &TypeKind,
) -> Result<(i32, Member), CodegenError> {
    let field = layout.projected_field(index).ok_or_else(|| {
        CodegenError::Internal(format!(
            "field index {index} out of range: `{kind}` has {} fields",
            layout.fields.len()
        ))
    })?;
    let member = match &field.repr {
        FieldRepr::Scalar(scalar) => Member::Word(
            crate::codegen::cranelift::value_locals::scalar_type(*scalar),
        ),
        FieldRepr::Value(inner) => Member::Inline(inner.clone()),
        FieldRepr::OptionTag => {
            return Err(CodegenError::Internal(format!(
                "field {index} of `{kind}` is an optional's tag, which no projection reaches"
            )))
        }
    };
    Ok((field.offset as i32, member))
}

/// Compute byte offset and Cranelift type for a field within an aggregate.
///
/// An optional held as its payload's own pointer is refused: it has no box,
/// its word is the payload, and no offset into the payload reaches it — an
/// answer of 0 would read the payload's own first word as the payload.
pub fn field_layout(
    local_type: &TypeKind,
    field_idx: usize,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Result<(i32, Member), CodegenError> {
    let ptr_size = ptr_ty.bytes() as i32;
    if let Some(layout) = held_value_layout(local_type, scope) {
        return value_member(&layout, field_idx, local_type);
    }
    match local_type {
        TypeKind::Tuple(element_exprs) => placed_field(
            tuple_payload_layout(element_exprs, scope, ptr_ty),
            field_idx,
            local_type,
        ),
        TypeKind::Option(_) => boxed_optional_field_layout(local_type, field_idx, scope, ptr_ty),
        // An open type parameter names no definition, however the program
        // spells its own types.
        TypeKind::Custom(name, None) if scope.is_parameter(name) => {
            Ok(((field_idx as i32) * ptr_size, Member::Word(ptr_ty)))
        }
        TypeKind::Custom(name, type_args) => {
            if let Some(layout) = vector_payload_layout(name, type_args.as_deref(), ptr_ty) {
                return placed_field(layout, field_idx, local_type);
            }
            custom_field_layout(name, type_args.as_deref(), field_idx, scope, ptr_ty)
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
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => Ok(((field_idx as i32) * ptr_size, Member::Word(ptr_ty))),
    }
}

/// Field `field_idx` of the optional `kind` held in a box: a word of the box.
/// One held as its payload's own pointer has no box and is refused.
fn boxed_optional_field_layout(
    kind: &TypeKind,
    field_idx: usize,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Result<(i32, Member), CodegenError> {
    if is_nullable_pointer_optional(kind, scope) {
        return Err(CodegenError::Internal(format!(
            "field {field_idx} of `{kind:?}` has no offset: the optional is its payload's own \
             pointer, with no box to hold the payload"
        )));
    }
    Ok((
        (field_idx as i32) * ptr_ty.bytes() as i32,
        Member::Word(ptr_ty),
    ))
}

/// Where each field of a struct, tuple or compiler-known vector of `kind`
/// sits in its payload, and how many bytes the payload needs; `None` for any
/// other kind.
///
/// This is the one authority for those payloads: the aggregate that builds a
/// value, every field read and write, equality, and the drop and clone paths
/// all place a field by it, so the bytes a value is built with are the bytes it
/// is read from. Fields are laid out from the declaration, each at its natural
/// alignment in declaration order, at the instantiation `kind` names; a tuple
/// keeps its element count in the word ahead of its first element.
pub fn aggregate_layout(
    kind: &TypeKind,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Option<PayloadLayout> {
    match kind {
        TypeKind::Tuple(element_exprs) => Some(tuple_payload_layout(element_exprs, scope, ptr_ty)),
        // An open type parameter names no definition, however the program
        // spells its own types.
        TypeKind::Custom(name, None) if scope.is_parameter(name) => None,
        TypeKind::Custom(name, type_args) => {
            custom_aggregate_layout(name, type_args.as_deref(), scope, ptr_ty)
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
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => None,
    }
}

/// [`aggregate_layout`] of `Custom(name, type_args)`: a compiler-known vector,
/// a struct, or an alias of either.
fn custom_aggregate_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Option<PayloadLayout> {
    if let Some(layout) = vector_payload_layout(name, type_args, ptr_ty) {
        return Some(layout);
    }
    match scope.definitions.get(name)? {
        TypeDefinition::Struct(struct_def) => {
            Some(struct_payload_layout(struct_def, type_args, scope, ptr_ty))
        }
        TypeDefinition::Alias(_) => {
            let written = TypeKind::Custom(name.to_string(), type_args.map(<[_]>::to_vec));
            let target = resolve_alias(&written, scope.definitions)?;
            aggregate_layout(&target.kind, scope, ptr_ty)
        }
        TypeDefinition::Enum(_)
        | TypeDefinition::Class(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Trait(_) => None,
    }
}

/// The payload of a compiler-known vector written with its component type
/// (`Vec3<f32>`): its components one after another. `None` for any other
/// name, a program's own `Vec3` among them.
fn vector_payload_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    ptr_ty: CraneliftType,
) -> Option<PayloadLayout> {
    let dim = crate::ast::types::vec_dim(name)?;
    let ExpressionKind::Type(component, _) = &type_args?.first()?.node else {
        return None;
    };
    let component = Slot::of(translate_type_kind(&component.kind, ptr_ty));
    Some(place_fields(0, (0..dim).map(|_| component.clone())))
}

/// The payload of a tuple held as a reference-counted allocation: the
/// element count in the first word, then each element at the type its type
/// argument names.
fn tuple_payload_layout(
    element_exprs: &[Expression],
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> PayloadLayout {
    let elements = element_exprs
        .iter()
        .map(|element| Slot::element(element, scope, ptr_ty));
    place_fields(ptr_ty.bytes() as i32, elements)
}

/// The payload of a struct at the instantiation `type_args` names.
fn struct_payload_layout(
    struct_def: &StructDefinition,
    type_args: Option<&[Expression]>,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> PayloadLayout {
    let fields = struct_def.fields.iter().map(|(_name, field_ty, _vis)| {
        struct_field_slot(struct_def, field_ty, type_args, scope, ptr_ty)
    });
    place_fields(0, fields)
}

/// Field `field_idx` of `layout`, the payload of a value of `kind`. An index
/// past the last field names no field, and no offset answers it.
fn placed_field(
    layout: PayloadLayout,
    field_idx: usize,
    kind: &TypeKind,
) -> Result<(i32, Member), CodegenError> {
    layout.fields.get(field_idx).cloned().ok_or_else(|| {
        CodegenError::Internal(format!(
            "field index {field_idx} out of range: `{kind}` has {} fields",
            layout.fields.len()
        ))
    })
}

/// Lays `slots` out after `header` bytes, each at its natural alignment in
/// order, and pads the whole to the widest alignment among the fields and the
/// header word.
fn place_fields(header: i32, slots: impl IntoIterator<Item = Slot>) -> PayloadLayout {
    let mut fields = Vec::new();
    let mut offset = header;
    let mut max_align = header.max(1);
    for slot in slots {
        max_align = max_align.max(slot.align);
        offset = align_to(offset, slot.align);
        fields.push((offset, slot.member));
        offset += slot.size;
    }
    PayloadLayout {
        fields,
        size: align_to(offset, max_align) as u32,
    }
}

fn custom_field_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Result<(i32, Member), CodegenError> {
    let type_definitions = scope.definitions;
    let ptr_size = ptr_ty.bytes() as i32;
    let Some(def) = type_definitions.get(name) else {
        // Type not found — assume pointer-sized fields
        return Ok(((field_idx as i32) * ptr_size, Member::Word(ptr_ty)));
    };
    Ok(match def {
        TypeDefinition::Struct(struct_def) => {
            let written = TypeKind::Custom(name.to_string(), type_args.map(<[_]>::to_vec));
            let layout = struct_payload_layout(struct_def, type_args, scope, ptr_ty);
            return placed_field(layout, field_idx, &written);
        }
        TypeDefinition::Enum(enum_def) => {
            enum_field_layout(enum_def, type_args, field_idx, scope, ptr_ty)
        }
        TypeDefinition::Alias(_) => {
            let written = TypeKind::Custom(name.to_string(), type_args.map(<[_]>::to_vec));
            return match resolve_alias(&written, type_definitions) {
                Some(target) => field_layout(&target.kind, field_idx, scope, ptr_ty),
                None => Ok(((field_idx as i32) * ptr_size, Member::Word(ptr_ty))),
            };
        }
        TypeDefinition::Class(_) => class_field_layout(name, type_args, field_idx, scope, ptr_ty),
        TypeDefinition::Generic(_) | TypeDefinition::Trait(_) => {
            ((field_idx as i32) * ptr_size, Member::Word(ptr_ty))
        }
    })
}

/// The slot a struct field occupies at the instantiation `type_args` names.
///
/// A field spelled as the struct's type parameter is laid out at its argument,
/// the way a generic class's field is: the constructor stores the argument's
/// value at its own width, so a read must find it at that width too.
fn struct_field_slot(
    struct_def: &StructDefinition,
    field_ty: &crate::ast::types::Type,
    type_args: Option<&[Expression]>,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> Slot {
    let stored = member_type_at(struct_def.generics.as_deref(), type_args, field_ty);
    Slot::member(&stored.kind, scope, ptr_ty)
}

/// Where field `field_idx` of an enum value sits: the discriminant is field 0,
/// and payload field `k` of whichever variant the value holds is field `k + 1`.
///
/// A read of a payload field does not know which variant it reads — the match
/// arm that reached it does, but the projection only carries the index — so
/// the offset must not depend on the variant. Every field therefore occupies
/// one slot of [`enum_payload_slot_size`], the same for all variants of the
/// enum at this instantiation. The type returned is a pointer-sized word; a
/// reader loads at the width of the binding it fills.
fn enum_field_layout(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> (i32, Member) {
    let slot = enum_payload_slot_size(enum_def, type_args, scope, ptr_ty) as i32;
    ((field_idx as i32) * slot, Member::Word(ptr_ty))
}

/// The width of every slot in an enum value: the discriminant's and each
/// payload field's.
///
/// It is the widest payload any variant carries at this instantiation, and
/// never narrower than a pointer, so a 128-bit payload gets a slot it fits in
/// and every other enum keeps pointer-sized slots. A payload held as its
/// bytes sits inline and takes its own size. The width is a multiple of the
/// widest alignment among the payloads, so every slot is aligned. A payload
/// spelled as a type parameter takes the width of the argument bound to it;
/// one with no bound argument is pointer-sized.
///
/// This is the one authority for enum payload offsets: construction, the
/// field reads and writes of a match, and the drop path all lay a value out
/// by it, so none of them can disagree about where a field lives.
pub fn enum_payload_slot_size(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> u32 {
    let word = Slot::of(ptr_ty);
    let (size, align) = enum_def
        .variants
        .values()
        .flatten()
        .map(|field_ty| {
            let stored = enum_payload_field_kind(enum_def, &field_ty.kind, type_args);
            Slot::member(&stored, scope, ptr_ty)
        })
        .fold((word.size, word.align), |(size, align), slot| {
            (size.max(slot.size), align.max(slot.align))
        });
    align_to(size, align) as u32
}

/// The kind an enum payload field declared as `declared` stores at the
/// instantiation `type_args`: a payload spelled as a type parameter takes the
/// argument bound to it, and any other payload is stored as declared. A
/// parameter with no bound argument is returned unresolved.
///
/// The slot width above, and the drop path's choice of which fields to
/// release, both resolve payloads through this, so the offsets a value is
/// written at and the offsets it is released from cannot disagree.
pub fn enum_payload_field_kind(
    enum_def: &EnumDefinition,
    declared: &TypeKind,
    type_args: Option<&[Expression]>,
) -> TypeKind {
    let declared =
        crate::ast::types::Type::new(declared.clone(), crate::error::syntax::Span::default());
    member_type_at(enum_def.generics.as_deref(), type_args, &declared).kind
}

/// Where each field of a heap payload sits, and how much memory the fields
/// need.
///
/// Both answers come from one walk so they cannot disagree. Sizing a payload
/// from anything other than the fields it holds — the widths of the values a
/// constructor happens to hand over, say — gives an object whose allocation is
/// smaller than the offsets written into it, and the write past the end lands
/// in whatever the allocator put next.
pub struct PayloadLayout {
    /// Offset and Cranelift type of each field, in declaration order.
    pub fields: Vec<(i32, Member)>,
    /// Bytes the payload occupies, counting any header ahead of its first
    /// field: a class's vtable slot, a tuple's element count.
    pub size: u32,
}

/// Lay out one class instance's payload, its fields in the order
/// [`field_types`] lists them. See [`PayloadLayout`].
pub fn class_payload_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> PayloadLayout {
    let ptr_size = ptr_ty.bytes() as i32;
    // Class layout: [header: 16 bytes (block size + RC)][vtable_ptr][field0][field1]...
    // Offset 0 holds the vtable pointer — null for a class without a vtable —
    // on every class, not only one that dispatches: a body compiled for a base
    // runs on instances of every class extending it, and a subclass that
    // dispatches while its base does not must still find each inherited field
    // where the base's bodies read it.
    let field_kinds = class_field_kinds(name, type_args, scope.definitions);
    let fields = field_kinds
        .iter()
        .map(|field_kind| Slot::member(field_kind, scope, ptr_ty));
    place_fields(ptr_size, fields)
}

/// The kind each field of an instance of `name` at `type_args` stores, in the
/// order the class lays them out, as [`field_types`] answers it.
fn class_field_kinds(
    name: &str,
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> Vec<TypeKind> {
    field_types(type_definitions, name, type_args)
        .unwrap_or_default()
        .into_iter()
        .map(|ty| ty.kind)
        .collect()
}

fn class_field_layout(
    name: &str,
    type_args: Option<&[Expression]>,
    field_idx: usize,
    scope: LayoutScope<'_>,
    ptr_ty: CraneliftType,
) -> (i32, Member) {
    let layout = class_payload_layout(name, type_args, scope, ptr_ty);
    match layout.fields.get(field_idx) {
        Some(placed) => placed.clone(),
        // An index past the last field belongs to no declared field. The slot
        // after the payload is the closest thing to an answer, and a caller
        // reaching it is already asking about a field the class does not have.
        None => (layout.size as i32, Member::Word(ptr_ty)),
    }
}

/// Compute total size of an aggregate for stack slot allocation.
///
/// Returns the size in bytes needed to represent the given aggregate type
/// on the stack. For structs, this is the sum of field sizes. For enums,
/// it is the discriminant plus the largest variant payload.
pub fn aggregate_size(
    local_type: &TypeKind,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes();
    match local_type {
        TypeKind::Tuple(element_exprs) => {
            tuple_payload_layout(
                element_exprs,
                LayoutScope::program(type_definitions),
                ptr_ty,
            )
            .size
        }
        TypeKind::Custom(name, type_args) => {
            custom_aggregate_size(name, type_args.as_deref(), type_definitions, ptr_ty)
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
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => ptr_size,
    }
}

/// Size of a `Custom(name, _)` aggregate by dispatching on the resolved
/// `TypeDefinition`. Unknown names and definitions that carry no on-stack
/// payload (classes, traits, generics) fall back to a pointer slot.
fn custom_aggregate_size(
    name: &str,
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let ptr_size = ptr_ty.bytes();
    match type_definitions.get(name) {
        Some(TypeDefinition::Struct(struct_def)) => {
            struct_aggregate_size(struct_def, type_args, type_definitions, ptr_ty)
        }
        Some(TypeDefinition::Enum(enum_def)) => {
            enum_aggregate_size(enum_def, type_args, type_definitions, ptr_ty)
        }
        Some(TypeDefinition::Alias(alias_def)) => {
            aggregate_size(&alias_def.template.kind, type_definitions, ptr_ty)
        }
        None
        | Some(TypeDefinition::Generic(_))
        | Some(TypeDefinition::Class(_))
        | Some(TypeDefinition::Trait(_)) => ptr_size,
    }
}

/// Size of a struct aggregate: its payload, padded to at least a pointer's
/// alignment.
fn struct_aggregate_size(
    struct_def: &StructDefinition,
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let scope = LayoutScope::program(type_definitions);
    let payload = struct_payload_layout(struct_def, type_args, scope, ptr_ty).size as i32;
    align_to(payload, ptr_ty.bytes() as i32) as u32
}

/// Size of an enum aggregate: the discriminant slot followed by as many
/// payload slots as the variant with the most fields carries, each slot as wide
/// as [`enum_payload_slot_size`] says.
fn enum_aggregate_size(
    enum_def: &EnumDefinition,
    type_args: Option<&[Expression]>,
    type_definitions: &HashMap<String, TypeDefinition>,
    ptr_ty: CraneliftType,
) -> u32 {
    let max_fields = enum_def.variants.values().map(Vec::len).max().unwrap_or(0);
    let scope = LayoutScope::program(type_definitions);
    enum_payload_slot_size(enum_def, type_args, scope, ptr_ty) * (1 + max_fields as u32)
}
