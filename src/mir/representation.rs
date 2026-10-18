// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a value of a type is represented at the top level, as far as a
//! compiled body can tell: a word of some width and register class, counted or
//! not; a value held as its bytes; or an optional of some layers. Each type
//! written inside it is a representation of its own, compared on its own.
//!
//! A body shared by every instantiation of a generic type reads each type
//! parameter as one uncounted word. Comparing the representation of every
//! type the body handles at an instantiation's arguments with its
//! representation at that fallback is what decides whether the instantiation
//! may share the body: two types alike here are compiled alike.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::mir::option_layout::{LayerRepr, OptionalLayers};
use crate::mir::rc::is_counted_in;
use crate::type_checker::element_layout::{element_layout, ElementLayout};
use crate::type_checker::value_class::{
    held_value_layout, value_class, LayoutScope, ScalarKind, ValueClass, ValueLayout,
};

/// How a value of one type is represented at the top level: how it is held,
/// and how a collection lays it out as an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Representation {
    held: Held,
    element: ElementLayout,
}

/// How a value is held, at the top level.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// One word: its width in bytes, whether it is a float, whether it counts.
    Word {
        bytes: u32,
        is_float: bool,
        is_counted: bool,
    },
    /// A value held as its bytes.
    Bytes(ValueLayout),
    /// An optional, layer by layer, outermost first.
    Optional(Vec<LayerRepr>),
}

impl Representation {
    /// The representation of a value of `kind`, read against `scope`.
    pub fn of(kind: &TypeKind, scope: LayoutScope<'_>) -> Self {
        Self {
            held: held(kind, scope),
            element: element_layout(kind, scope),
        }
    }

    /// This representation with reference counting left out: what a body
    /// that only hands values on to the runtime sees. A boxed optional is then
    /// one word like any other pointer.
    pub fn without_counting(self) -> Self {
        let held = match self.held {
            Held::Word {
                bytes, is_float, ..
            } => Held::Word {
                bytes,
                is_float,
                is_counted: false,
            },
            Held::Optional(layers) if !layers.contains(&LayerRepr::Inline) => Held::Word {
                bytes: WORD_BYTES,
                is_float: false,
                is_counted: false,
            },
            other @ (Held::Optional(_) | Held::Bytes(_)) => other,
        };
        Self {
            held,
            element: self.element,
        }
    }
}

/// The width of a pointer or of `int`, in bytes.
const WORD_BYTES: u32 = 8;

/// How a value of `kind` is held at the top level.
fn held(kind: &TypeKind, scope: LayoutScope<'_>) -> Held {
    let layers = OptionalLayers::of(kind, scope);
    if layers.is_optional() {
        return Held::Optional(layers.reprs());
    }
    if let Some(layout) = held_value_layout(kind, scope) {
        return Held::Bytes(layout);
    }
    let (bytes, is_float) = match value_class(kind, scope) {
        ValueClass::Scalar(scalar) => (scalar.size(), matches!(scalar, ScalarKind::Float(_))),
        ValueClass::Bitwise(_) | ValueClass::Word(_) | ValueClass::Managed => (WORD_BYTES, false),
    };
    Held::Word {
        bytes,
        is_float,
        is_counted: is_counted_in(kind, scope),
    }
}

/// The types `kind` is written with: a tuple's elements, a type's arguments,
/// a collection's element, key and value, an optional's payload, a function
/// type's parameters and result.
pub fn written_type_arguments(kind: &TypeKind) -> Box<dyn Iterator<Item = &Type> + '_> {
    match kind {
        TypeKind::Tuple(elements) | TypeKind::Custom(_, Some(elements)) => {
            Box::new(elements.iter().filter_map(written_type))
        }
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Array(element, _) => {
            Box::new(written_type(element).into_iter())
        }
        TypeKind::Map(key, value) => {
            Box::new(written_type(key).into_iter().chain(written_type(value)))
        }
        TypeKind::Option(payload) => Box::new(std::iter::once(payload.as_ref())),
        TypeKind::Function(function) => Box::new(
            function
                .params
                .iter()
                .filter_map(|param| written_type(&param.typ))
                .chain(function.return_type.as_deref().and_then(written_type)),
        ),
        TypeKind::Custom(_, None)
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
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::OneOf(_)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_) => Box::new(std::iter::empty()),
    }
}

/// The type a type expression writes, or `None` for a value expression.
fn written_type(expr: &Expression) -> Option<&Type> {
    let ExpressionKind::Type(ty, _) = &expr.node else {
        return None;
    };
    Some(ty.as_ref())
}
