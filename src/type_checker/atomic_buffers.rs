// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The refusal of an atomic operation whose buffer cannot hold atomics.
//!
//! GPU atomics hold 32-bit integers only, so the buffer an atomic builtin acts
//! on must be `Array<Atomic<i32>, N>` or `Array<Atomic<u32>, N>`. The type
//! checker refuses any other buffer type it can see; lowering refuses the ones
//! only an instantiation reveals. Both say it in the words built here.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{array_element_and_size, Type, TypeKind};
use crate::type_checker::diagnostics::spelled;

/// What an atomic call on the wrong buffer is told.
pub(crate) struct AtomicBufferMismatch {
    pub message: String,
    pub help: String,
    /// The buffer type the operation needs, as source spells it.
    pub expected: String,
    /// The buffer type it was given, as source spells it.
    pub found: String,
}

/// The refusal of `builtin` acting on `buffer`, an argument of type `ty`.
pub(crate) fn atomic_buffer_mismatch(
    builtin: &str,
    buffer: &Expression,
    ty: &Type,
) -> AtomicBufferMismatch {
    let found = spelled(ty);
    let named = buffer_name(buffer);
    let subject = named
        .map(|name| format!("`{name}`"))
        .unwrap_or_else(|| "the buffer".to_string());
    let message = format!(
        "`{builtin}` needs a buffer of `Array<Atomic<i32>, N>` or `Array<Atomic<u32>, N>`, \
         but {subject} is `{found}`"
    );
    let help = match (named, atomic_replacement(ty)) {
        (Some(name), Some(replacement)) => {
            format!("declare `{name}` as `{replacement}` (atomics hold 32-bit integers only)")
        }
        (None, Some(replacement)) => {
            format!("pass a buffer declared as `{replacement}` (atomics hold 32-bit integers only)")
        }
        (_, None) => "pass a buffer declared with `gpu var buf = Array<Atomic<i32>, N>()` \
             (atomics hold 32-bit integers only)"
            .to_string(),
    };
    AtomicBufferMismatch {
        message,
        help,
        expected: "Array<Atomic<i32>, N> or Array<Atomic<u32>, N>".to_string(),
        found,
    }
}

/// The name a buffer argument is bound to, when it is a plain name.
fn buffer_name(buffer: &Expression) -> Option<&str> {
    match &buffer.node {
        ExpressionKind::Identifier(name, _) => Some(name),
        _ => None,
    }
}

/// The atomic buffer type that keeps an array's size: its element becomes
/// `Atomic<u32>` when it was `u32`, and `Atomic<i32>` otherwise.
fn atomic_replacement(ty: &Type) -> Option<String> {
    let (element, size) = array_element_and_size(&ty.kind)?;
    let unsigned = matches!(
        &element.node,
        ExpressionKind::Type(element, _) if matches!(element.kind, TypeKind::U32)
    );
    let integer = if unsigned { "u32" } else { "i32" };
    Some(format!("Array<Atomic<{integer}>, {}>", size.node))
}
