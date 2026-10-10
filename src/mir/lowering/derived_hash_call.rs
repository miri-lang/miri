// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of `value.hash()` on a value whose type declares no `hash`.
//!
//! Every type `==` compares structurally answers `hash()` with a hash derived
//! from the same parts, the way `==` derives its comparison: the type checker
//! admits the call and this lowering builds it. A call on a type that declares
//! `hash` — or on a trait-typed receiver, which reaches the method of the
//! instance it holds — is an ordinary method call and is left to dispatch.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind, HASH_METHOD_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Operand, Place, Rvalue, Statement, StatementKind};
use crate::type_checker::context::{class_method_declaration, TypeDefinition};
use std::collections::{HashMap, HashSet};

use super::expression::structural_hash::emit_structural_hash;
use super::{lower_expression, LoweringContext};

/// Lowers `receiver.hash()` when the receiver's type derives its hash, into
/// `dest` when the call has one. Returns `None` for any other call.
pub(super) fn try_lower_derived_hash_call(
    ctx: &mut LoweringContext,
    span: &Span,
    receiver: &Expression,
    method: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(method_name, _) = &method.node else {
        return Ok(None);
    };
    if method_name != HASH_METHOD_NAME || !args.is_empty() {
        return Ok(None);
    }
    let Some(receiver_ty) = ctx.recorded_type(receiver.id) else {
        return Ok(None);
    };
    if declares_hash(ctx, &receiver_ty) {
        return Ok(None);
    }
    // The receiver is only read: a temporary it was built into, such as a
    // constructed struct, is released once its hash is taken.
    let watermark = ctx.body.local_decls.len();
    let value = lower_expression(ctx, receiver, None)?;
    let temporary = match &value {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Some(place.local)
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
    };
    let hash = Operand::Copy(Place::new(emit_structural_hash(
        ctx,
        *span,
        &receiver_ty.kind,
        value,
    )?));
    if let Some(local) = temporary {
        ctx.emit_temp_drop(local, watermark, *span);
    }
    let Some(dest) = dest else {
        return Ok(Some(hash));
    };
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(dest.clone(), Rvalue::Use(hash)),
        span: *span,
    });
    Ok(Some(Operand::Copy(dest.clone())))
}

/// Whether a call of `hash` on a value of `ty` is an ordinary method call: the
/// type declares the method, or is a trait whose instances do.
fn declares_hash(ctx: &LoweringContext, ty: &Type) -> bool {
    let TypeKind::Custom(name, _) = &ty.kind else {
        return false;
    };
    let definitions = ctx.type_checker.type_definitions();
    match definitions.get(name) {
        Some(TypeDefinition::Class(_)) => {
            class_method_declaration(name, HASH_METHOD_NAME, definitions).is_some()
        }
        Some(TypeDefinition::Enum(enum_def)) => enum_def.methods.contains_key(HASH_METHOD_NAME),
        Some(TypeDefinition::Trait(_)) => trait_declares_hash(name, definitions),
        Some(TypeDefinition::Struct(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_))
        | None => false,
    }
}

/// Whether the trait `name`, or one it extends, declares `hash`, so a value
/// held at it reaches the method of the instance it holds.
fn trait_declares_hash(name: &str, definitions: &HashMap<String, TypeDefinition>) -> bool {
    let mut pending = vec![name.to_string()];
    let mut seen = HashSet::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }
        if let Some(TypeDefinition::Trait(trait_def)) = definitions.get(&current) {
            if trait_def.methods.contains_key(HASH_METHOD_NAME) {
                return true;
            }
            pending.extend(trait_def.parent_traits.iter().cloned());
        }
    }
    false
}
