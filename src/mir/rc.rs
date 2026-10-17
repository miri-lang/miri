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
use crate::type_checker::value_class::{value_class, LayoutScope, ValueClass, WordRole};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

/// Whether a bitwise value is reference counted.
///
/// A struct, tuple or optional the value-class authority calls bitwise is
/// still built in a heap allocation of its own and released like any other
/// aggregate, so it is counted. This is the one place that says so.
const BITWISE_VALUE_IS_COUNTED: bool = true;

/// Decides whether a value of a type is reference counted, by its value class
/// read against the program's type table.
///
/// A body carries one, shared with every other body lowered from the same
/// check, and asks it at the body's own type parameters.
#[derive(Clone)]
pub struct RcClassifier {
    definitions: Rc<HashMap<String, TypeDefinition>>,
    is_bitwise_counted: bool,
}

impl Default for RcClassifier {
    fn default() -> Self {
        Self::new(Rc::default())
    }
}

impl RcClassifier {
    /// A classifier that counts a bitwise value the way every program is
    /// compiled.
    pub fn new(definitions: Rc<HashMap<String, TypeDefinition>>) -> Self {
        Self {
            definitions,
            is_bitwise_counted: BITWISE_VALUE_IS_COUNTED,
        }
    }

    /// A classifier that holds every bitwise value as its bytes, uncounted.
    ///
    /// No program is compiled this way. It is the representation the
    /// value-representation verifier guards, and lets a hand-built body be
    /// checked against it.
    pub fn with_uncounted_bitwise_values(definitions: Rc<HashMap<String, TypeDefinition>>) -> Self {
        Self {
            definitions,
            is_bitwise_counted: false,
        }
    }

    /// Whether a value of `kind`, written in a body that leaves `type_params`
    /// open, is reference counted.
    pub fn is_counted(&self, kind: &TypeKind, type_params: &HashSet<String>) -> bool {
        !is_never_counted(kind)
            && self.is_counted_class(&value_class(
                kind,
                LayoutScope::new(&self.definitions, type_params),
            ))
    }

    /// Whether a value of class `class` is reference counted.
    fn is_counted_class(&self, class: &ValueClass) -> bool {
        match class {
            ValueClass::Scalar(_) | ValueClass::Word(WordRole::Parameter | WordRole::Untracked) => {
                false
            }
            ValueClass::Word(WordRole::NullablePointer) | ValueClass::Managed => true,
            ValueClass::Bitwise(_) => self.is_bitwise_counted,
        }
    }
}

impl fmt::Debug for RcClassifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RcClassifier")
            .field("definitions", &self.definitions.len())
            .field("is_bitwise_counted", &self.is_bitwise_counted)
            .finish()
    }
}

/// Two classifiers are equal when they count bitwise values alike and read
/// one table, or both read an empty one. Type definitions have no equality of
/// their own, so two copies of one program's table are told apart.
impl PartialEq for RcClassifier {
    fn eq(&self, other: &Self) -> bool {
        self.is_bitwise_counted == other.is_bitwise_counted
            && (Rc::ptr_eq(&self.definitions, &other.definitions)
                || (self.definitions.is_empty() && other.definitions.is_empty()))
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
