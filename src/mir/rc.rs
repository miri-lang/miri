// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared reference-counting helpers for MIR.
//!
//! [`RcClassifier`] is the single authority on whether a value of a type is
//! reference counted: the lowering context, Perceus, the passes that move or
//! elide references and the verifier all ask it, through [`Body::is_counted`].
//! It answers from the value-class authority over the declared type.
//!
//! [`Body::is_counted`]: crate::mir::Body::is_counted

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{TypeKind, SELF_TYPE_NAME};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::value_class::{
    held_value_layout, value_class, LayoutScope, ValueClass, ValueLayout, WordRole,
};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

/// Decides whether a value of a type is reference counted, by its value class
/// read against the program's type table.
///
/// A body carries one, shared with every other body lowered from the same
/// check, and asks it at the body's own type parameters.
#[derive(Clone)]
pub struct RcClassifier {
    definitions: Rc<HashMap<String, TypeDefinition>>,
}

impl Default for RcClassifier {
    fn default() -> Self {
        Self::new(Rc::default())
    }
}

impl RcClassifier {
    /// A classifier reading `definitions`.
    pub fn new(definitions: Rc<HashMap<String, TypeDefinition>>) -> Self {
        Self { definitions }
    }

    /// Whether a value of `kind`, written in a body that leaves `type_params`
    /// open, is reference counted.
    pub fn is_counted(&self, kind: &TypeKind, type_params: &HashSet<String>) -> bool {
        is_counted_in(kind, LayoutScope::new(&self.definitions, type_params))
    }

    /// The layout a value of `kind`, written in a body that leaves
    /// `type_params` open, is held in as its bytes; `None` when it is held as
    /// a word.
    pub fn value_layout(
        &self,
        kind: &TypeKind,
        type_params: &HashSet<String>,
    ) -> Option<ValueLayout> {
        if is_never_counted(kind) {
            return None;
        }
        match value_class(kind, LayoutScope::new(&self.definitions, type_params)) {
            ValueClass::Bitwise(layout) if layout.is_held_as_bytes() => Some(layout),
            ValueClass::Bitwise(_)
            | ValueClass::Scalar(_)
            | ValueClass::Word(_)
            | ValueClass::Managed => None,
        }
    }

    /// The type table every type is classified against.
    pub fn definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.definitions
    }
}

impl fmt::Debug for RcClassifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RcClassifier")
            .field("definitions", &self.definitions.len())
            .finish()
    }
}

/// Two classifiers are equal when they read one table, or both read an
/// empty one. Type definitions have no equality of their own, so two copies
/// of one program's table are told apart.
impl PartialEq for RcClassifier {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.definitions, &other.definitions)
            || (self.definitions.is_empty() && other.definitions.is_empty())
    }
}

/// Whether a value of `kind`, read against `scope`, is reference counted.
pub fn is_counted_in(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    !is_never_counted(kind)
        && match value_class(kind, scope) {
            ValueClass::Scalar(_) | ValueClass::Word(WordRole::Parameter | WordRole::Untracked) => {
                false
            }
            ValueClass::Word(WordRole::NullablePointer) | ValueClass::Managed => true,
            ValueClass::Bitwise(layout) => !layout.is_held_as_bytes(),
        }
}

/// The kinds no value of which is counted, whatever their value class.
///
/// - `void` holds no value.
/// - The native `Result` and `Future` kinds hold none in a body: the type
///   checker spells every `Result` as the `Result` enum, and an `async` call
///   is typed at its result, so a `Future` local is only ever an annotation.
/// - A `Linear` wrapper is used exactly once by the type checker's rule, and
///   no release path counts it.
/// - `Self`, unsubstituted, stands for the receiver's type the way a type
///   parameter stands for its argument.
fn is_never_counted(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Void | TypeKind::Result(_, _) | TypeKind::Future(_) | TypeKind::Linear(_) => true,
        TypeKind::Custom(name, None) => name == SELF_TYPE_NAME,
        TypeKind::Custom(_, Some(_))
        | TypeKind::Int
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
        | TypeKind::String
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Error
        | TypeKind::OneOf(_)
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Option(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Function(_)
        | TypeKind::Meta(_) => false,
    }
}

/// Returns true if a field/element type is managed (heap-allocated, needs DecRef
/// on drop). Vector types (Vec2/3/4) are value types stored inline — when held
/// as a collection element or aggregate field they are raw bytes, not a managed
/// pointer, so they must never be DecRef'd here.
///
/// A function value is a closure allocation of its own wherever it is held, so
/// a collection element or an aggregate field holding one releases it. A
/// closure's captures are one-word slots and are decided by
/// [`is_word_slot_managed`] instead.
///
/// The native `Result` and `Future` kinds answer `false` because no value of
/// either reaches a body: the type checker spells every `Result` as the
/// `Result` enum (a `Custom`, managed here) and `Body::validate` refuses a
/// local left at the native kind, while an `async` call is typed at its
/// result, so a `Future` slot is only ever written as an annotation and never
/// holds a value.
pub fn is_field_managed(kind: &TypeKind) -> bool {
    if let TypeKind::Custom(name, args) = kind {
        // Inline scalar/vector element wrappers (`Vec*`, `Atomic<scalar>`) are
        // stored by value, never reference-counted — exclude them from the
        // managed-element drop path. Each is recognized by its component, as
        // the field-layout path recognizes it, so a user type that merely
        // reuses the name stays managed and its allocation is released.
        //
        // A class or struct field, a tuple element, and an enum or optional
        // payload hold a vector as a pointer instead; the drop path for those
        // slots asks `is_word_slot_managed`.
        if crate::ast::types::vec_type_dim(kind).is_some()
            || is_builtin_atomic(name, args.as_deref())
        {
            return false;
        }
    }
    matches!(
        kind,
        TypeKind::Option(_)
            | TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Function(_)
            | TypeKind::Custom(_, _)
    )
}

/// Whether `name<args>` is the compiler-known atomic element type.
///
/// That type is always written with its scalar component (`Atomic<u32>`), and
/// is stored as that scalar. A user type that only shares the name carries no
/// component, and is an ordinary heap object like any other class or struct.
pub fn is_builtin_atomic(name: &str, args: Option<&[Expression]>) -> bool {
    name == crate::ast::types::ATOMIC_TYPE_NAME && atomic_component(args).is_some()
}

/// The scalar component `args` names for the atomic element type, if any.
pub fn atomic_component(args: Option<&[Expression]>) -> Option<&Expression> {
    args?
        .first()
        .filter(|arg| matches!(arg.node, ExpressionKind::Type(_, _)))
}

/// Returns true if a value held in a one-word slot — an optional's payload, an
/// enum's payload, a tuple element, a field of a class or struct, or a
/// closure's capture — is managed.
///
/// Such a slot holds its value as one word laid out at the field's own type,
/// never as inline bytes, so a vector reaches it as the pointer to an allocation
/// of its own — where the inline-layout reasoning behind [`is_field_managed`]
/// does not hold. Freeing the holder without releasing that pointer leaks the
/// vector.
pub fn is_word_slot_managed(kind: &TypeKind) -> bool {
    is_field_managed(kind) || crate::ast::types::vec_type_dim(kind).is_some()
}

/// Whether a one-word slot holding a value of `kind`, read against `scope`,
/// holds a counted reference its holder releases.
///
/// A value held as its bytes sits inline in its holder instead: it is no
/// reference, and its holder's release has nothing to release in it.
pub fn is_slot_counted(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    is_word_slot_managed(kind) && held_value_layout(kind, scope).is_none()
}

/// Whether a collection element of `kind`, read against `scope`, is a counted
/// reference the collection releases; see [`is_field_managed`]. A value held
/// as its bytes is laid out inline and is not.
pub fn is_element_counted(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    is_field_managed(kind) && held_value_layout(kind, scope).is_none()
}
