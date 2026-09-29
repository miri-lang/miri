// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which set elements and map keys are matched through an equality the
//! compiler synthesizes, and the symbol that equality is compiled under.
//!
//! A struct's `==` walks its fields and an enum's compares its variant and
//! payload, or calls the `equals` it declares, so their bytes — an address, or
//! a bare discriminant — say nothing about whether two of them are equal. A set
//! or map of them is matched through a body lowered from that same `==`, one
//! per instantiation, which the container calls as it calls a class's
//! `equals`.

use crate::ast::types::{vec_dim, BuiltinCollectionKind, Type, TypeKind};
use crate::mir::instantiation::monomorphized_arguments;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::HashMap;

/// The symbol of the synthesized equality a set of `ty` elements, or a map of
/// `ty` keys, matches through; `None` when `ty` is matched some other way, or
/// is written at arguments no instantiation spells.
pub fn synthesized_equality_symbol(
    ty: &TypeKind,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<Symbol> {
    let TypeKind::Custom(name, arg_exprs) = ty else {
        return None;
    };
    // A compiler-known vector is stored inline in its slot, whose bytes are
    // its components: matching them is already `==`.
    if vec_dim(name).is_some() {
        return None;
    }
    let generics = match type_defs.get(name.as_str()) {
        Some(TypeDefinition::Struct(definition)) => &definition.generics,
        Some(TypeDefinition::Enum(definition)) => &definition.generics,
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => return None,
    };
    let arity = generics.as_ref().map_or(0, Vec::len);
    let args = match arg_exprs.as_deref() {
        Some(exprs) if arity > 0 => monomorphized_arguments(exprs, arity, type_defs)?,
        _ if arity > 0 => return None,
        _ => Vec::new(),
    };
    Some(Symbol::type_thunk(ThunkKind::Equals, name, &args))
}

/// The element and key types of every set and map `ty` spells, at any depth,
/// with the optionals around each opened: the types a container of them
/// matches.
pub fn matched_element_types(ty: &Type, found: &mut Vec<Type>) {
    match &ty.kind {
        TypeKind::Custom(name, Some(args)) => {
            let arg_types: Vec<&Type> = args
                .iter()
                .filter_map(|expr| match &expr.node {
                    crate::ast::expression::ExpressionKind::Type(arg, _) => Some(arg.as_ref()),
                    _ => None,
                })
                .collect();
            let matched = match BuiltinCollectionKind::from_name(name) {
                Some(BuiltinCollectionKind::Set | BuiltinCollectionKind::Map) => {
                    arg_types.first().copied()
                }
                Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array) | None => None,
            };
            if let Some(element) = matched {
                found.push(without_optionals(element).clone());
            }
            for arg in arg_types {
                matched_element_types(arg, found);
            }
        }
        TypeKind::Option(inner) | TypeKind::Linear(inner) => matched_element_types(inner, found),
        TypeKind::Tuple(elements) => {
            for element in elements {
                if let crate::ast::expression::ExpressionKind::Type(inner, _) = &element.node {
                    matched_element_types(inner, found);
                }
            }
        }
        _ => {}
    }
}

/// `ty` with every optional around it opened.
fn without_optionals(mut ty: &Type) -> &Type {
    while let TypeKind::Option(inner) = &ty.kind {
        ty = inner;
    }
    ty
}
