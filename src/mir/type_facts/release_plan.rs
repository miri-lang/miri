// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What freeing a value of any type does once its count reaches zero, and
//! which captures a closure's environment releases.
//!
//! A value is freed by the runtime helper of its shape, after the counted
//! references it holds are released: a list's or array's elements, a map's
//! values, a tuple's slots, a boxed optional's payload. A named type is
//! released through the drop thunk its [`super::DropPlan`] describes. A
//! [`ReleasePlan`] names the shape and every counted member, so a backend
//! emits the release and decides nothing.

use super::drop_plan::CountedSlot;
use crate::ast::type_arguments::TypeArgKind;
use crate::ast::type_arguments::TypeArgument;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::option_layout::{resolve_alias, LayoutScope};
use crate::mir::rc::is_slot_counted;

/// How a value of one type is freed once its count reaches zero.
#[derive(Debug, Clone, PartialEq)]
pub enum ReleasePlan {
    /// A map: the value of each occupied entry is released when the values
    /// are counted, then the runtime frees the map and its keys.
    Map { counted_value: Option<Type> },
    /// A set: the runtime frees it and its elements.
    Set,
    /// A list: each element is released when the elements are counted, then
    /// the runtime frees the list.
    List { counted_element: Option<Type> },
    /// An array: each element is released when the elements are counted, then
    /// the runtime frees the array.
    Array { counted_element: Option<Type> },
    /// A tuple: each counted slot is released, then the block is freed.
    Tuple { counted: Vec<CountedSlot> },
    /// An optional. Held as its payload's own pointer, it is released as the
    /// payload; boxed, its payload is released when counted, then the box is
    /// freed. Which of the two it is, the optional's layout decides.
    Optional {
        payload: Type,
        payload_counted: bool,
    },
    /// A string: the runtime frees it.
    String,
    /// A struct, class, enum or trait value: released through a drop thunk
    /// (see [`super::DropThunk`] and [`super::Release`]).
    Named {
        name: String,
        args: Option<Vec<TypeArgument>>,
    },
    /// A closure: its destructor releases its captures, then it is freed.
    Closure,
    /// Anything else: the block is freed.
    Block,
}

/// A value released with a type argument that names no type, so the counted
/// references it holds cannot be found; releasing it anyway would leak them.
#[derive(Debug, Clone, PartialEq)]
pub struct UnresolvedArgument(pub String);

/// The plan for freeing a value of `kind`, read in `scope`. An alias is read
/// as the type it stands for.
pub fn release_plan(
    kind: &TypeKind,
    scope: LayoutScope<'_>,
) -> Result<ReleasePlan, UnresolvedArgument> {
    let resolved = resolve_alias(kind, scope.definitions);
    let kind = resolved.as_ref().map_or(kind, |target| &target.kind);
    let counted = |ty: &Type| is_slot_counted(&ty.kind, scope).then(|| ty.clone());
    let plan = match kind.as_builtin_collection() {
        Some(BuiltinCollectionKind::Map) => ReleasePlan::Map {
            counted_value: counted(argument_type(kind, 1)?),
        },
        Some(BuiltinCollectionKind::Set) => ReleasePlan::Set,
        Some(BuiltinCollectionKind::List) => ReleasePlan::List {
            counted_element: counted(argument_type(kind, 0)?),
        },
        Some(BuiltinCollectionKind::Array) => ReleasePlan::Array {
            counted_element: counted(argument_type(kind, 0)?),
        },
        None => plan_of_other(kind, scope)?,
    };
    Ok(plan)
}

/// The plan for a value that is no built-in collection.
fn plan_of_other(
    kind: &TypeKind,
    scope: LayoutScope<'_>,
) -> Result<ReleasePlan, UnresolvedArgument> {
    let plan = match kind {
        TypeKind::Tuple(elements) => ReleasePlan::Tuple {
            counted: counted_slots(elements, kind, scope)?,
        },
        TypeKind::Option(payload) => ReleasePlan::Optional {
            payload: (**payload).clone(),
            payload_counted: is_slot_counted(&payload.kind, scope),
        },
        TypeKind::String => ReleasePlan::String,
        TypeKind::Custom(name, args) => ReleasePlan::Named {
            name: name.clone(),
            args: args.clone(),
        },
        TypeKind::Function(_) => ReleasePlan::Closure,
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
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::OneOf(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::Generic(_, _, _) => ReleasePlan::Block,
    };
    Ok(plan)
}

/// The counted slots of the tuple `kind` holding `elements`.
fn counted_slots(
    elements: &[TypeArgument],
    kind: &TypeKind,
    scope: LayoutScope<'_>,
) -> Result<Vec<CountedSlot>, UnresolvedArgument> {
    let mut counted = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let ty = resolved_type(Some(element), kind)?;
        if is_slot_counted(&ty.kind, scope) {
            counted.push(CountedSlot {
                index,
                ty: ty.clone(),
            });
        }
    }
    Ok(counted)
}

/// Whether a slot holding a value of `kind` holds a counted reference, which
/// replacing or discarding the slot's value releases.
pub fn releases_slot(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    is_slot_counted(kind, scope)
}

/// The positions, among a closure's captures of `capture_types` in
/// environment order, whose values the closure's destructor releases. A
/// closure none of whose captures is counted has no destructor.
pub fn released_captures<'t>(
    capture_types: impl IntoIterator<Item = &'t TypeKind>,
    scope: LayoutScope<'_>,
) -> Vec<usize> {
    (capture_types.into_iter().enumerate())
        .filter(|(_, kind)| is_slot_counted(kind, scope))
        .map(|(position, _)| position)
        .collect()
}

/// The type argument at `position` of the built-in collection `kind`.
fn argument_type(kind: &TypeKind, position: usize) -> Result<&Type, UnresolvedArgument> {
    let argument = if let TypeKind::List(element)
    | TypeKind::Array(element, _)
    | TypeKind::Set(element) = kind
    {
        (position == 0).then_some(&**element)
    } else if let TypeKind::Map(key, value) = kind {
        [&**key, &**value].get(position).copied()
    } else if let TypeKind::Custom(_, Some(args)) = kind {
        args.get(position)
    } else {
        None
    };
    resolved_type(argument, kind)
}

/// The type `argument` of `kind` spells. Lowering hands a release every type
/// argument substituted; one that is missing or spelled as anything but a
/// type means it did not, and is refused.
fn resolved_type<'k>(
    argument: Option<&'k TypeArgument>,
    kind: &TypeKind,
) -> Result<&'k Type, UnresolvedArgument> {
    match argument.map(|argument| &argument.node) {
        Some(TypeArgKind::Type(ty, _)) => Ok(ty),
        Some(_) | None => Err(UnresolvedArgument(format!(
            "a `{kind}` value is released with an unresolved type argument, so its \
             counted elements cannot be found"
        ))),
    }
}
