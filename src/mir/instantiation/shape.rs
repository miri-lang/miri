// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The shape of a type as instantiation sees it: the constructor it is built
//! with and the types it is built from, how deep those nest, and which type
//! parameters it still leaves open.
//!
//! Lowering bounds the instantiations a program may need by this shape, and
//! the drop-instantiation closure the backend reads bounds itself by the same
//! measure, so a type one side refuses is refused by the other.

use super::instantiation_argument;
use crate::ast::expression::Expression;
use crate::ast::types::{Type, TypeKind};
use crate::mir::symbol::token::{type_kind_to_mangle_str, MAX_TOKEN_DEPTH};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::generics::extract_value_generic_kind;
use std::borrow::Cow;
use std::collections::HashMap;

/// How many type constructors deep one instance may nest, counting the class
/// itself: `Vec<List<List<String>>>` is three deep.
///
/// Held under the symbol mangler's own depth, so every instance within it has
/// a per-instantiation name, and a body is never shared across instances
/// because the mangler ran out of depth.
pub const MAX_INSTANCE_TYPE_DEPTH: usize = 32;

const _: () = assert!(MAX_INSTANCE_TYPE_DEPTH < MAX_TOKEN_DEPTH);

/// How many type constructors deep an instance at `args` nests, counting its
/// class: one more than its deepest argument. Past
/// [`MAX_INSTANCE_TYPE_DEPTH`] the count stops one level beyond it, which is
/// all a caller asking whether the bound is passed needs.
pub fn instance_type_depth(args: &[Type]) -> usize {
    1 + deepest_argument_depth(args)
}

/// How many type constructors deep the deepest of `args` nests, stopping one
/// level past [`MAX_INSTANCE_TYPE_DEPTH`].
pub fn deepest_argument_depth<'t>(args: impl IntoIterator<Item = &'t Type>) -> usize {
    args.into_iter()
        .map(|arg| type_depth(arg, MAX_INSTANCE_TYPE_DEPTH))
        .max()
        .unwrap_or(0)
}

/// How many type constructors deep `ty` nests, a type built from nothing
/// counting none, looking no further than `budget` levels down.
fn type_depth(ty: &Type, budget: usize) -> usize {
    let parts = constructor_parts(ty).1;
    if parts.is_empty() {
        return 0;
    }
    match budget.checked_sub(1) {
        Some(rest) => {
            1 + parts
                .iter()
                .map(|part| type_depth(part, rest))
                .max()
                .unwrap_or(0)
        }
        None => 1,
    }
}

/// Whether `ty` names a type parameter no substitution has bound, anywhere
/// inside it: a type still open is not yet an instantiation at all.
pub fn mentions_open_parameter(ty: &Type, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    mentions_open_parameter_within(ty, type_defs, MAX_TOKEN_DEPTH)
}

fn mentions_open_parameter_within(
    ty: &Type,
    type_defs: &HashMap<String, TypeDefinition>,
    budget: usize,
) -> bool {
    let is_open = if let TypeKind::Custom(name, None) = &ty.kind {
        matches!(type_defs.get(name), None | Some(TypeDefinition::Generic(_)))
    } else {
        matches!(ty.kind, TypeKind::Generic(..))
    };
    let Some(rest) = budget.checked_sub(1) else {
        return is_open;
    };
    is_open
        || constructor_parts(ty)
            .1
            .iter()
            .any(|part| mentions_open_parameter_within(part, type_defs, rest))
}

/// `ty` as its outermost constructor and the types it is built from, in
/// order. A value generic's size and every type built from nothing are
/// leaves, their constructor the token the mangler spells them with. A
/// built-in constructor is spelled with a leading digit, which no declared
/// type's name can start with, so a class named `tuple` is never read as one.
pub(crate) fn constructor_parts(ty: &Type) -> (Cow<'_, str>, Vec<Type>) {
    let leaf = || (type_kind_to_mangle_str(&ty.kind), Vec::new());
    if extract_value_generic_kind(&ty.kind).is_some() {
        return leaf();
    }
    let arguments = |exprs: &[&Expression]| -> Vec<Type> {
        exprs
            .iter()
            .filter_map(|expr| instantiation_argument(expr))
            .collect()
    };
    match &ty.kind {
        TypeKind::Custom(name, args) => {
            let args: Vec<&Expression> = args.iter().flatten().collect();
            (Cow::Borrowed(name.as_str()), arguments(&args))
        }
        TypeKind::Generic(name, _, _) => (Cow::Borrowed(name.as_str()), Vec::new()),
        TypeKind::Option(inner) => (Cow::Borrowed("0option"), vec![(**inner).clone()]),
        TypeKind::Linear(inner) => (Cow::Borrowed("0linear"), vec![(**inner).clone()]),
        TypeKind::Meta(inner) => (Cow::Borrowed("0meta"), vec![(**inner).clone()]),
        TypeKind::Tuple(elements) => {
            let elements: Vec<&Expression> = elements.iter().collect();
            (Cow::Borrowed("0tuple"), arguments(&elements))
        }
        TypeKind::List(element) => (Cow::Borrowed("0list"), arguments(&[element])),
        TypeKind::Set(element) => (Cow::Borrowed("0set"), arguments(&[element])),
        TypeKind::Future(element) => (Cow::Borrowed("0future"), arguments(&[element])),
        TypeKind::Array(element, size) => (Cow::Borrowed("0array"), arguments(&[element, size])),
        TypeKind::Map(key, value) => (Cow::Borrowed("0map"), arguments(&[key, value])),
        TypeKind::Result(ok, err) => (Cow::Borrowed("0result"), arguments(&[ok, err])),
        TypeKind::Function(function) => {
            let mut parts: Vec<&Expression> =
                function.params.iter().map(|param| &*param.typ).collect();
            parts.extend(function.return_type.as_deref());
            (Cow::Borrowed("0fn"), arguments(&parts))
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
        | TypeKind::Void
        | TypeKind::Error => leaf(),
    }
}

/// The name of every type parameter `ty` leaves open, anywhere inside it, in
/// the order they are met — what [`mentions_open_parameter`] finds, by name.
pub fn open_parameter_names(ty: &Type, type_defs: &HashMap<String, TypeDefinition>) -> Vec<String> {
    let mut names = Vec::new();
    collect_open_parameter_names(ty, type_defs, MAX_TOKEN_DEPTH, &mut names);
    names
}

fn collect_open_parameter_names(
    ty: &Type,
    type_defs: &HashMap<String, TypeDefinition>,
    budget: usize,
    names: &mut Vec<String>,
) {
    let open_name = if let TypeKind::Custom(name, None) = &ty.kind {
        matches!(type_defs.get(name), None | Some(TypeDefinition::Generic(_))).then_some(name)
    } else if let TypeKind::Generic(name, _, _) = &ty.kind {
        Some(name)
    } else {
        None
    };
    names.extend(open_name.cloned());
    let Some(rest) = budget.checked_sub(1) else {
        return;
    };
    for part in constructor_parts(ty).1 {
        collect_open_parameter_names(&part, type_defs, rest, names);
    }
}
