// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The element-layout authority: how a collection holds an element of one
//! type, and how that element travels to the runtime.
//!
//! The layout is decided against a [`LayoutScope`] — the program's type table
//! and the type parameters the code asking leaves open — so it can read what
//! a named type is, not only how it is spelled: a value held as its bytes is
//! laid out inline by those bytes.

use crate::ast::types::{
    atomic_inner_kind, inline_element_layout, scalar_width, TypeKind, VALUE_WORD_BYTES,
};
use crate::type_checker::value_class::{held_value_layout, LayoutScope};

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
    word_or_value_layout(kind, scope)
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

/// The layout of an element that is not a scalar as spelled.
///
/// A value held as its bytes is held inline, one value after another at its
/// own alignment, and handed over as the address of those bytes; its operand
/// is the value, whose address is taken to hand it over. Every other such
/// element is held in one value word: a reference, a type parameter, a name
/// that resolves to a scalar.
fn word_or_value_layout(kind: &TypeKind, scope: LayoutScope<'_>) -> ElementLayout {
    match held_value_layout(kind, scope) {
        Some(layout) => ElementLayout {
            payload: i64::from(layout.size),
            stride: i64::from(layout.size.next_multiple_of(layout.align.max(1))),
            is_address: false,
        },
        None => ElementLayout::held_by_value(VALUE_WORD_BYTES),
    }
}
