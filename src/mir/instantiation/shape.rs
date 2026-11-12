// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The shape of a type as instantiation sees it: the constructor it is built
//! with and the types it is built from, and which type parameters it still
//! leaves open.
//!
//! How deep a type nests and how many types it is built from, and the bounds
//! on both, are stated once in [`crate::type_checker::instantiation_limits`]
//! and read from there by every stage that bounds an instantiation.

use crate::ast::type_arguments::{TypeArgKind, TypeArgument, ValueArg};
use crate::ast::types::{Type, TypeKind};
use crate::mir::symbol::token::{type_kind_to_mangle_str, MAX_TOKEN_DEPTH};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::generics::extract_value_generic_kind;
use crate::type_checker::instantiation_limits::{self, Constructor};
pub use crate::type_checker::instantiation_limits::{
    arguments_type_size, deepest_argument_depth, instance_type_depth, MAX_INSTANCE_TYPE_DEPTH,
    MAX_INSTANCE_TYPE_SIZE,
};
use std::borrow::Cow;
use std::collections::HashMap;

// Every instance within the depth bound has a per-instantiation name, so a
// body is never shared across instances because the mangler ran out of depth.
const _: () = assert!(MAX_INSTANCE_TYPE_DEPTH < MAX_TOKEN_DEPTH);

/// Whether `ty` names a type parameter no substitution has bound, anywhere
/// inside it: a type still open is not yet an instantiation at all.
pub fn mentions_open_parameter(ty: &Type, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    let is_open =
        |name: &str| matches!(type_defs.get(name), None | Some(TypeDefinition::Generic(_)));
    mentions_type_parameter(ty, &is_open, false)
}

/// Whether `ty` names a type parameter anywhere inside it: a
/// [`TypeKind::Generic`], or a bare name `is_parameter` accepts.
///
/// The walk looks no deeper than the symbol mangler names a type. A type
/// nested past that answers `past_depth` for what lies below it, so a caller
/// that must not miss a parameter passes `true`.
pub fn mentions_type_parameter(
    ty: &Type,
    is_parameter: &dyn Fn(&str) -> bool,
    past_depth: bool,
) -> bool {
    mentions_type_parameter_within(ty, is_parameter, past_depth, MAX_TOKEN_DEPTH)
}

fn mentions_type_parameter_within(
    ty: &Type,
    is_parameter: &dyn Fn(&str) -> bool,
    past_depth: bool,
    budget: usize,
) -> bool {
    let names_one_here = if let TypeKind::Custom(name, None) = &ty.kind {
        is_parameter(name)
    } else {
        matches!(ty.kind, TypeKind::Generic(..))
    };
    let Some(rest) = budget.checked_sub(1) else {
        return names_one_here || past_depth;
    };
    names_one_here
        || constructor_parts(ty)
            .1
            .iter()
            .any(|part| mentions_type_parameter_within(part, is_parameter, past_depth, rest))
}

/// `ty` as its outermost constructor and the types it is built from, in
/// order ([`instantiation_limits::constructor_parts`]), a leaf's constructor
/// spelled as the token the mangler spells it with.
pub(crate) fn constructor_parts(ty: &Type) -> (Cow<'_, str>, Vec<Type>) {
    let (constructor, parts) = instantiation_limits::constructor_parts(ty);
    let head = match constructor {
        Constructor::Named(name) => Cow::Borrowed(name),
        Constructor::Leaf(kind) => type_kind_to_mangle_str(kind),
    };
    (head, parts)
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
    // A value argument still written as an expression (`Size`, `Size + 1`)
    // leaves open every value parameter it reads.
    if let Some(value) = extract_value_generic_kind(&ty.kind) {
        names.extend(value_parameter_names(value));
        return;
    }
    let open_name = if let TypeKind::Custom(name, None) = &ty.kind {
        matches!(type_defs.get(name), None | Some(TypeDefinition::Generic(_))).then_some(name)
    } else if let TypeKind::Generic(name, _) = &ty.kind {
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

/// Whether `arg`, at `position` among the arguments of the generic type
/// `name`, is that type's own parameter declared at the same position — an
/// argument inference never bound, left spelled as the parameter
/// (`E` in the `Result<String, E>` of `let r = Result.Ok("s")`).
pub fn is_own_parameter_at(
    type_defs: &HashMap<String, TypeDefinition>,
    name: &str,
    position: usize,
    arg: &Type,
) -> bool {
    let Some(declared) = type_defs
        .get(name)
        .and_then(TypeDefinition::generics)
        .and_then(|generics| generics.get(position))
    else {
        return false;
    };
    let spelled = if let TypeKind::Generic(param, _) = &arg.kind {
        Some(param)
    } else if let TypeKind::Custom(param, None) = &arg.kind {
        Some(param)
    } else {
        None
    };
    spelled == Some(&declared.name)
}

/// `args` with every argument inference left unbound
/// ([`is_own_parameter_at`]) spelled one way, as the open parameter
/// `Generic(name)`: the checker writes an unbound argument either as that or as
/// the bare name `Custom(name, None)`, and the two would otherwise name two
/// drop functions for one instantiation.
pub fn with_unbound_arguments_normalized(
    type_defs: &HashMap<String, TypeDefinition>,
    name: &str,
    args: &[Type],
) -> Vec<Type> {
    args.iter()
        .enumerate()
        .map(|(position, arg)| {
            let TypeKind::Custom(param, None) = &arg.kind else {
                return arg.clone();
            };
            if !is_own_parameter_at(type_defs, name, position, arg) {
                return arg.clone();
            }
            Type::new(
                TypeKind::Generic(param.clone(), crate::ast::bounds::Bounds::none()),
                arg.span,
            )
        })
        .collect()
}

/// Whether `args` instantiate the generic type `name` at arguments that are
/// concrete, except for positions inference left unbound
/// ([`is_own_parameter_at`]), with at least one concrete argument and no
/// unbound parameter read anywhere else.
///
/// A value built at such an instantiation stores nothing at an unbound
/// position — a value there would have bound it — so its drop function
/// releases what the concrete arguments hold and skips the rest.
pub fn is_partially_bound_instantiation(
    type_defs: &HashMap<String, TypeDefinition>,
    name: &str,
    args: &[Type],
) -> bool {
    let Some(arity) = type_defs
        .get(name)
        .and_then(TypeDefinition::generics)
        .map(<[_]>::len)
    else {
        return false;
    };
    if args.len() != arity {
        return false;
    }
    let mut concrete = 0;
    for (position, arg) in args.iter().enumerate() {
        if is_own_parameter_at(type_defs, name, position, arg) {
            continue;
        }
        if !super::has_a_monomorphized_spelling(&arg.kind)
            || mentions_open_parameter(arg, type_defs)
        {
            return false;
        }
        concrete += 1;
    }
    concrete > 0
}

/// The parameters an unfolded value argument reads — `Size` in `Size`, or in
/// `Size + 1`; none for a type argument.
pub fn value_parameter_names(arg: &TypeArgument) -> Vec<String> {
    fn names(value: &ValueArg, found: &mut Vec<String>) {
        match value {
            ValueArg::Int(_) | ValueArg::OutOfRange { .. } => {}
            ValueArg::Name(name) => found.push(name.clone()),
            ValueArg::Binary(_, left, right) => {
                names(left, found);
                names(right, found);
            }
        }
    }
    match &arg.node {
        TypeArgKind::Value(value) => {
            let mut found = Vec::new();
            names(value, &mut found);
            found
        }
        TypeArgKind::Type(..) => Vec::new(),
    }
}
