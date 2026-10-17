// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The layout authority for optional values.
//!
//! An optional whose payload is itself a pointer to a reference-counted
//! allocation can spell its absence as a null payload pointer, with no box of
//! its own. Every other optional layer — a scalar or inline-vector payload, or
//! an outer layer of a nested optional — needs a box, so that the layer's
//! absence and the inner value's absence stay distinguishable.
//!
//! [`OptionalLayers`] is the one place that decides how many layers of a
//! (possibly nested) optional are boxed, and [`payload_is_nullable_pointer`]
//! the one place that decides which payloads are such pointers. Every site that
//! builds, reads or releases an optional asks these, so they agree.
//!
//! Both decide through [`resolve_alias`]: an alias is laid out as the type it
//! names at the arguments it is written with, so `Id<Node>` (`type Id<T> is
//! T`) is a `Node`, `Opt<Node>` (`type Opt<T> is T?`) is a `Node?`, and an
//! optional of an alias naming `Node?` is a `Node??`.

use crate::ast::expression::Expression;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::instantiation::{apply_generic_sub, instantiation_argument};
use crate::mir::rc::is_field_managed;
use crate::type_checker::context::TypeDefinition;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// What the layout of a type is decided against: the program's type table and
/// the type parameters the body being laid out leaves open.
///
/// Inside a generic body a bare name that is one of its parameters means that
/// parameter, even where the program defines a type of the same name, so a
/// standard-library body over `V` is not laid out as the program's own `V`.
#[derive(Debug, Clone, Copy)]
pub struct LayoutScope<'d> {
    pub definitions: &'d HashMap<String, TypeDefinition>,
    pub type_params: &'d HashSet<String>,
}

impl<'d> LayoutScope<'d> {
    /// The scope of a body that leaves `type_params` open.
    pub fn new(
        definitions: &'d HashMap<String, TypeDefinition>,
        type_params: &'d HashSet<String>,
    ) -> Self {
        Self {
            definitions,
            type_params,
        }
    }

    /// The scope of code with no type parameter open.
    pub fn program(definitions: &'d HashMap<String, TypeDefinition>) -> Self {
        static NO_PARAMETERS: OnceLock<HashSet<String>> = OnceLock::new();
        Self::new(definitions, NO_PARAMETERS.get_or_init(HashSet::new))
    }

    /// Whether the bare name `name` is one of the open type parameters.
    pub fn is_parameter(&self, name: &str) -> bool {
        self.type_params.contains(name)
    }

    /// The alias `kind` resolves to, unless `kind` names an open parameter.
    fn resolve_alias(&self, kind: &TypeKind) -> Option<Type> {
        match kind {
            TypeKind::Custom(name, None) if self.is_parameter(name) => None,
            _ => resolve_alias(kind, self.definitions),
        }
    }
}

/// How a possibly nested optional type is laid out.
///
/// For `Node??` with `Node` a class, the innermost layer is the nullable
/// `Node` pointer and the outer layer is boxed: `boxed` is 1,
/// `innermost_nullable_pointer` is true and `value` is `Node`. For `int??`
/// both layers are boxed. A type that is not optional has no layers.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionalLayers<'t> {
    /// The number of optional layers that hold their payload in a box.
    pub boxed: usize,
    /// Whether the innermost optional layer is the payload pointer itself,
    /// null when absent.
    pub innermost_nullable_pointer: bool,
    /// The innermost non-optional kind, with every alias on the way resolved:
    /// borrowed from the type asked about unless an alias had to be expanded.
    pub value: Cow<'t, TypeKind>,
}

impl<'t> OptionalLayers<'t> {
    /// The layout of `kind`, where `definitions` is the program's type table.
    ///
    /// An alias is peeled like the type it names, so an alias naming an
    /// optional adds that optional's layers.
    pub fn of(kind: &'t TypeKind, scope: LayoutScope<'_>) -> Self {
        let (layers, value) = peel_optionals(kind, scope);
        let innermost_nullable_pointer = layers > 0 && payload_is_nullable_pointer(&value, scope);
        Self {
            boxed: layers - usize::from(innermost_nullable_pointer),
            innermost_nullable_pointer,
            value,
        }
    }
}

/// How many optionals wrap `kind`, seen through aliases, and the value they
/// wrap. The value is borrowed from `kind` unless an alias had to be expanded.
///
/// Each alias expansion is followed by an optional to peel; aliases that only
/// reach each other through optionals form an infinite type, which the bound
/// on expansions stops at the alias reached.
fn peel_optionals<'t>(kind: &'t TypeKind, scope: LayoutScope<'_>) -> (usize, Cow<'t, TypeKind>) {
    let mut layers = 0;
    let mut value = Cow::Borrowed(kind);
    let mut expansions_left = scope.definitions.len();
    loop {
        value = match value {
            Cow::Borrowed(TypeKind::Option(inner)) => Cow::Borrowed(&inner.kind),
            Cow::Owned(TypeKind::Option(inner)) => Cow::Owned(inner.kind),
            other => match scope.resolve_alias(&other) {
                Some(target) if expansions_left > 0 => {
                    expansions_left -= 1;
                    value = Cow::Owned(target.kind);
                    continue;
                }
                Some(_) | None => return (layers, other),
            },
        };
        layers += 1;
    }
}

/// Whether a value of `kind` is an optional held as its payload's own pointer,
/// null when absent, rather than as a box: an optional, or an alias naming
/// one, whose only layer is a nullable pointer.
pub fn is_nullable_pointer_optional(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    let layers = OptionalLayers::of(kind, scope);
    layers.boxed == 0 && layers.innermost_nullable_pointer
}

/// The type the alias `kind` names stands for, with the alias's parameters
/// replaced by the arguments `kind` is written with, followed through chained
/// aliases. `None` when `kind` names no alias, or when the chain is a cycle.
///
/// This is the one reading of an alias: lowering spells a declared type by it,
/// and the optional layout and the release path decide by it, so the type an
/// alias stands for cannot be laid out one way and released another.
pub fn resolve_alias(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> Option<Type> {
    let mut resolved = alias_target(kind, definitions)?;
    for _ in 0..definitions.len() {
        match alias_target(&resolved.kind, definitions) {
            Some(next) => resolved = next,
            None => return Some(resolved),
        }
    }
    None
}

/// One step of [`resolve_alias`]: the template of the alias `kind` names, at
/// the arguments `kind` is written with. A parameter left without an argument
/// stays as written.
fn alias_target(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> Option<Type> {
    let TypeKind::Custom(name, args) = kind else {
        return None;
    };
    let Some(TypeDefinition::Alias(alias)) = definitions.get(name) else {
        return None;
    };
    let substitution: HashMap<String, Type> = alias
        .generics
        .iter()
        .flatten()
        .zip(args.iter().flatten())
        .filter_map(|(parameter, arg)| Some((parameter.name.clone(), instantiation_argument(arg)?)))
        .collect();
    Some(apply_generic_sub(&alias.template, &substitution))
}

/// Whether an optional holding a `kind` payload can be that payload's pointer,
/// null when absent.
///
/// True for a string, a built-in collection at its type arguments, a tuple, a
/// function value, and a name the type table defines as a class, struct, enum
/// or trait. An alias is judged as the type it names at its arguments, so an
/// alias naming an optional is an optional. False for every value held by value —
/// scalars, aliases to them, inline vectors and atomics — for an optional
/// (whose own absence must stay distinct from the outer one), for names that
/// denote no allocation (generic parameters, a bare name the scope leaves open as a
/// parameter, placeholders, a collection named
/// without arguments inside its own body), and for kinds no optional payload
/// is a reference-counted pointer of.
///
pub fn payload_is_nullable_pointer(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    match kind {
        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Function(_) => true,
        TypeKind::Custom(name, None) if scope.is_parameter(name) => false,
        TypeKind::Custom(name, args) => match resolve_alias(kind, scope.definitions) {
            Some(target) => payload_is_nullable_pointer(&target.kind, scope),
            None => {
                is_field_managed(kind) && names_allocation(name, args.as_deref(), scope.definitions)
            }
        },
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
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::OneOf(_) => false,
    }
}

/// Whether `name<args>` names a reference-counted allocation.
///
/// A built-in collection does only at its type arguments: named bare, it is
/// the unresolved self-reference inside the collection's own body. Any other
/// name is decided by its definition. An alias is resolved before it gets
/// here; one that does not resolve is a cycle, which names nothing.
fn names_allocation(
    name: &str,
    args: Option<&[Expression]>,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    if BuiltinCollectionKind::from_name(name).is_some() {
        return args.is_some();
    }
    match definitions.get(name) {
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Trait(_),
        ) => true,
        Some(TypeDefinition::Alias(_) | TypeDefinition::Generic(_)) | None => false,
    }
}
