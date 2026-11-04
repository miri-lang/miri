// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What releasing a value of a named type does once its count reaches zero.
//!
//! A struct, class or enum value is released through a drop thunk: it runs
//! the type's `fn drop(self)` hook, if it has one, releases every counted
//! field — for an enum, every counted payload of the variant the value holds —
//! and frees the block. Which fields are counted depends on the instantiation:
//! a field written `value T` holds a string in a `Box<String>` and a bare word
//! in a `Box<int>`. A [`DropPlan`] states all of it for one thunk, read from
//! the same field resolver every other stage reads, so a backend emits the
//! thunk from the plan and decides nothing.
//!
//! The plans are built once per drop thunk the program emits: the shared thunk
//! of every struct, class and enum, and the thunk of every instantiation in
//! the drop set.

use crate::ast::type_arguments::{type_argument, TypeArgument};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::instantiation::{field_types, member_type_at, shape::open_parameter_names};
use crate::mir::option_layout::LayoutScope;
use crate::mir::rc::is_slot_counted;
use crate::mir::symbol::Symbol;
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

/// How a value whose last reference is dropped is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Through the type's own drop thunk: the value is of exactly that type.
    Exact,
    /// Through the drop slot of the vtable the value points at, or the type's
    /// own drop thunk when it points at none: a class another class extends.
    ByRuntimeClass,
    /// Through the drop slot of the vtable the value points at: a trait, which
    /// has no drop thunk of its own.
    Trait,
}

impl Release {
    /// How a value whose static type is `name` is released.
    pub fn of(name: &str, definitions: &HashMap<String, TypeDefinition>) -> Self {
        match definitions.get(name) {
            Some(TypeDefinition::Trait(_)) => Release::Trait,
            _ if crate::mir::dispatch::released_by_runtime_class(name, definitions) => {
                Release::ByRuntimeClass
            }
            _ => Release::Exact,
        }
    }
}

/// One counted member a drop thunk releases: its position, and the type the
/// instance stores it at.
#[derive(Debug, Clone, PartialEq)]
pub struct CountedSlot {
    /// The field's position among the type's fields, or the payload's
    /// position within its variant.
    pub index: usize,
    /// The type the member holds in this instantiation.
    pub ty: Type,
}

/// The counted members a drop thunk releases.
#[derive(Debug, Clone, PartialEq)]
pub enum DroppedMembers {
    /// Every counted field of a struct or class, in layout order.
    Fields(Vec<CountedSlot>),
    /// Every variant of an enum carrying a counted payload, by discriminant,
    /// with its counted payloads.
    Variants(Vec<(usize, Vec<CountedSlot>)>),
}

impl DroppedMembers {
    /// Whether the thunk releases no member.
    pub fn is_empty(&self) -> bool {
        match self {
            DroppedMembers::Fields(fields) => fields.is_empty(),
            DroppedMembers::Variants(variants) => variants.is_empty(),
        }
    }
}

/// Everything one drop thunk does before it frees the block.
#[derive(Debug, Clone, PartialEq)]
pub struct DropPlan {
    /// The `fn drop(self)` hook the thunk runs first, if the type has one.
    pub hook: Option<Symbol>,
    /// The counted members the thunk releases after the hook.
    pub members: DroppedMembers,
}

impl DropPlan {
    /// Whether the thunk does anything beyond freeing the block: runs a hook
    /// or releases a member. A value whose plan does nothing is freed in place.
    pub fn releases_anything(&self) -> bool {
        self.hook.is_some() || !self.members.is_empty()
    }
}

/// The plan of the drop thunk of `name` at `args`; `None` for the shared
/// thunk, which reads every field as declared.
///
/// A field still written at a parameter the thunk leaves open names nothing
/// concrete, and the thunk cannot release it at a type it may not have: the
/// shared thunk of a generic type is reached only as a collection element's
/// release helper, where the value was released through its own
/// instantiation's thunk, so such a field is skipped.
pub(crate) fn drop_plan(
    definitions: &HashMap<String, TypeDefinition>,
    name: &str,
    args: Option<&[Type]>,
) -> DropPlan {
    let open = thunk_open_parameters(definitions, name, args);
    drop_plan_in(definitions, name, args, &open)
}

/// The plan for releasing a `name` value at `args` in code that leaves the
/// parameters `open` unbound: what a drop thunk does, read where the value is
/// released rather than inside the thunk.
pub(crate) fn drop_plan_in(
    definitions: &HashMap<String, TypeDefinition>,
    name: &str,
    args: Option<&[Type]>,
    open: &HashSet<String>,
) -> DropPlan {
    let scope = LayoutScope::new(definitions, open);
    let is_generic = definitions
        .get(name)
        .and_then(TypeDefinition::generics)
        .is_some();
    let arg_exprs: Vec<TypeArgument> = args
        .unwrap_or_default()
        .iter()
        .cloned()
        .map(type_argument)
        .collect();
    let counted = |index: usize, ty: Type| {
        let skipped = is_generic && is_unresolved_element(&ty.kind, definitions);
        (!skipped && is_slot_counted(&ty.kind, scope)).then_some(CountedSlot { index, ty })
    };
    let members = match definitions.get(name) {
        Some(TypeDefinition::Enum(def)) => {
            let type_args = args.map(|_| arg_exprs.as_slice());
            DroppedMembers::Variants(
                (def.variants.values().enumerate())
                    .filter_map(|(variant, payloads)| {
                        let counted: Vec<CountedSlot> = (payloads.iter().enumerate())
                            .filter_map(|(index, declared)| {
                                let ty =
                                    member_type_at(def.generics.as_deref(), type_args, declared);
                                counted(index, ty)
                            })
                            .collect();
                        (!counted.is_empty()).then_some((variant, counted))
                    })
                    .collect(),
            )
        }
        Some(TypeDefinition::Struct(_) | TypeDefinition::Class(_)) => DroppedMembers::Fields(
            (field_types(definitions, name, Some(&arg_exprs)).unwrap_or_default())
                .into_iter()
                .enumerate()
                .filter_map(|(index, ty)| counted(index, ty))
                .collect(),
        ),
        Some(TypeDefinition::Trait(_) | TypeDefinition::Alias(_) | TypeDefinition::Generic(_))
        | None => DroppedMembers::Fields(Vec::new()),
    };
    DropPlan {
        hook: crate::mir::lowering::dispatch_symbols::drop_hook_symbol(name, definitions),
        members,
    }
}

/// The type parameters the drop thunk of `name` at `args` reads its fields
/// at. The shared thunk of a generic type reads them at the parameters they
/// are declared in — the type's own and every ancestor's; a per-instantiation
/// thunk reads them at its arguments, which leave open only a parameter
/// inference never bound.
pub(crate) fn thunk_open_parameters(
    definitions: &HashMap<String, TypeDefinition>,
    name: &str,
    args: Option<&[Type]>,
) -> HashSet<String> {
    match args {
        None => declared_parameters(definitions, name),
        Some(args) => args
            .iter()
            .flat_map(|arg| open_parameter_names(arg, definitions))
            .collect(),
    }
}

/// Every type parameter `name` and the classes it extends declare.
fn declared_parameters(
    definitions: &HashMap<String, TypeDefinition>,
    name: &str,
) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut current = Some(name);
    // Bounded by the table's size: a circular `extends` is reported where
    // the class is declared, and must not hang the walk before that.
    for _ in 0..=definitions.len() {
        let Some(definition) = current.and_then(|name| definitions.get(name)) else {
            break;
        };
        names.extend(
            (definition.generics().unwrap_or_default().iter()).map(|generic| generic.name.clone()),
        );
        current = match definition {
            TypeDefinition::Class(class) => class.base_class.as_deref(),
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Trait(_) => None,
        };
    }
    names
}

/// Whether an element or member of type `kind` names no type its release
/// helper could be found for: a type parameter, an optional of one, or a name
/// the program defines no struct, class, enum, alias or trait for. Built-in
/// collections are always resolved: each has a shared release helper.
pub fn is_unresolved_element(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    match kind {
        TypeKind::Generic(_, _, _) => true,
        TypeKind::Option(payload) => is_unresolved_element(&payload.kind, definitions),
        TypeKind::Custom(name, _) => {
            if BuiltinCollectionKind::from_name(name).is_some() {
                return false;
            }
            match definitions.get(name) {
                None | Some(TypeDefinition::Generic(_)) => true,
                Some(_) => false,
            }
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
        | TypeKind::Tuple(_)
        | TypeKind::OneOf(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => false,
    }
}
