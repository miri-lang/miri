// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The element-layout authority: how a collection holds an element of one
//! type, and how that element travels to the runtime.
//!
//! The layout is decided against a [`LayoutScope`] — the program's type table
//! and the type parameters the code asking leaves open — so it can read what
//! a named type is, not only how it is spelled. Today a bitwise value is still
//! held as a reference in one value word; the scope is what lets it be laid
//! out by its own bytes instead.

use crate::ast::types::{
    atomic_inner_kind, inline_element_layout, scalar_width, TypeKind, VALUE_WORD_BYTES,
};
use crate::type_checker::value_class::{value_class, LayoutScope, ValueClass};

/// How a collection holds an element of one type and how that element travels
/// to the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementLayout {
    /// Bytes that are the element — what a runtime entry point is told to copy
    /// out of the address it is handed.
    pub payload: i64,
    /// Spacing between consecutive elements of a list or an array, which is
    /// what an index read strides by. Never less than `payload`.
    pub stride: i64,
    /// Whether the operand carrying the element is already the address of its
    /// bytes. An inline vector is; every other element is a value, whose
    /// address is taken to hand it over.
    pub is_address: bool,
}

impl ElementLayout {
    /// The layout of an element held by value in `bytes` bytes.
    fn held_by_value(bytes: i64) -> Self {
        Self {
            payload: bytes,
            stride: bytes,
            is_address: false,
        }
    }
}

/// The layout of a collection element of type `kind`, decided against `scope`.
///
/// This is the one place an element's width is decided. The stride a list is
/// allocated with, the payload each call hands the runtime, and whether that
/// call must take the element's address all come from here, so a store and the
/// read or lookup that must match it cannot disagree.
pub fn element_layout(kind: &TypeKind, scope: LayoutScope<'_>) -> ElementLayout {
    if let Some(inline) = inline_element_layout(kind) {
        return ElementLayout {
            payload: inline.payload,
            stride: inline.stride,
            is_address: true,
        };
    }
    if let Some(inner) = atomic_inner_kind(kind) {
        return element_layout(inner, scope);
    }
    if let Some(width) = held_scalar_bytes(kind) {
        return ElementLayout::held_by_value(width);
    }
    ElementLayout::held_by_value(word_held_bytes(kind, scope))
}

/// Byte width of an element held as the scalar it is spelled as, or `None`
/// for an element that is not one.
///
/// An `f16` element is the exception to its scalar width: a collection holds
/// it in a value word, which is what every host path that stores one writes.
fn held_scalar_bytes(kind: &TypeKind) -> Option<i64> {
    if matches!(kind, TypeKind::F16) {
        return Some(VALUE_WORD_BYTES);
    }
    scalar_width(kind)
}

/// Byte width of an element that is not a scalar as spelled.
///
/// Every such element is held in one value word: a reference, a type
/// parameter, a name that resolves to a scalar, and a bitwise value, which is
/// still built in an allocation of its own and stored by its pointer.
fn word_held_bytes(kind: &TypeKind, scope: LayoutScope<'_>) -> i64 {
    match value_class(kind, scope) {
        ValueClass::Bitwise(_) => VALUE_WORD_BYTES,
        ValueClass::Scalar(_) | ValueClass::Word(_) | ValueClass::Managed => VALUE_WORD_BYTES,
    }
}
