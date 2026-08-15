// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Value copies for aggregates that assign bitwise.
//!
//! A small all-primitive struct assigns by value: `var b = a` must give `b` its
//! own copy, so writing `b.x` leaves `a.x` alone. Such a struct is still stored
//! as a pointer to a heap block, so copying the operand alone would copy the
//! pointer and produce two names for one block. Rebuilding the aggregate from
//! its fields is what makes the copy a copy.
//!
//! Nested aggregate fields are rebuilt too. A field holding another struct holds
//! a pointer to it, so stopping at the top level would leave the inner block
//! shared and mutations through one name visible through the other.
//!
//! Enums are left alone. A variant payload is readable only by matching, which
//! binds the payload rather than a place to assign through, so two names for one
//! enum block cannot be told apart.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::place::PlaceElem;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, Statement, StatementKind};

/// Lowers the object a field access reads or writes through, as a place.
///
/// `a.x` has to reach the storage `a` names, for a read as much as for a write:
/// lowering `a` as a value would rebuild the whole aggregate and then project a
/// field out of the copy, which allocates per field access and — when assigning
/// — discards the write.
///
/// Anything that is not an aggregate assigning bitwise is lowered exactly as
/// before. A base lowered to `Move` is how a collection witnesses uniqueness for
/// copy-on-write, so widening this would turn in-place mutation into copying.
pub(crate) fn lower_projection_base(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Operand, LoweringError> {
    match value_aggregate_place(ctx, obj) {
        Some(place) => Ok(Operand::Copy(place)),
        None => crate::mir::lowering::expression::lower_expression(ctx, obj, None),
    }
}

/// Resolves `obj` to the place it names, when it names an aggregate that assigns
/// bitwise.
///
/// Walks a chain of field accesses (`outer.part.inner`) so a nested base is
/// reached by projection instead of by rebuilding each aggregate along the way.
/// Returns `None` for anything else, including a base whose type is reference
/// counted, whose lowering must stay exactly as it was.
fn value_aggregate_place(ctx: &mut LoweringContext, obj: &Expression) -> Option<Place> {
    match &obj.node {
        ExpressionKind::Identifier(name, _) => {
            let local = *ctx.variable_map.get(name.as_str())?;
            let ty = ctx.body.local_decls[local.0].ty.clone();
            value_aggregate_fields(ctx, &ty)?;
            Some(Place::new(local))
        }
        ExpressionKind::Member(inner, prop) => {
            let ty = ctx.type_checker.get_type(obj.id)?.clone();
            value_aggregate_fields(ctx, &ty)?;
            let inner_ty = ctx.type_checker.get_type(inner.id)?.clone();
            let TypeKind::Custom(type_name, _) = &inner_ty.kind else {
                return None;
            };
            let index = field_index(ctx, type_name, prop)?;
            let mut place = value_aggregate_place(ctx, inner)?;
            place.projection.push(PlaceElem::Field(index));
            Some(place)
        }
        _ => None,
    }
}

/// Returns the declaration order of the field `prop` names on struct `type_name`.
fn field_index(ctx: &LoweringContext, type_name: &str, prop: &Expression) -> Option<usize> {
    let ExpressionKind::Identifier(field_name, _) = &prop.node else {
        return None;
    };
    let crate::type_checker::context::TypeDefinition::Struct(definition) =
        ctx.type_checker.type_definitions().get(type_name)?
    else {
        return None;
    };
    definition
        .fields
        .iter()
        .position(|(name, _, _)| name == field_name.as_str())
}

/// Builds an independent copy of `place`, whose declared type is `ty`.
///
/// The copy is built directly into `dest` when the caller has one, so binding a
/// name to an existing aggregate does not route through an intermediate block
/// that then has to be released. Without a destination the copy lands in a
/// temporary, which the enclosing statement releases along with its other
/// temporaries.
///
/// Returns `None` when `ty` is not an aggregate that assigns bitwise, leaving
/// the caller to lower the reference however it normally would.
pub(crate) fn copy_value_aggregate(
    ctx: &mut LoweringContext,
    place: &Place,
    ty: &Type,
    dest: Option<Place>,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let Some(fields) = value_aggregate_fields(ctx, ty) else {
        return Ok(None);
    };

    let mut operands = Vec::with_capacity(fields.len());
    for (index, field_ty) in fields.iter().enumerate() {
        let mut field_place = place.clone();
        field_place.projection.push(PlaceElem::Field(index));
        let operand = match copy_value_aggregate(ctx, &field_place, field_ty, None, span)? {
            Some(nested) => nested,
            None => Operand::Copy(field_place),
        };
        operands.push(operand);
    }

    let target = match dest {
        Some(d) => d,
        None => {
            // No name refers to this copy, so nothing else would release it.
            let temp = ctx.push_temp(ty.clone(), span);
            ctx.register_scope_temp(temp);
            Place::new(temp)
        }
    };
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(AggregateKind::Struct(ty.clone()), operands),
        ),
        span,
    });
    Ok(Some(Operand::Copy(target)))
}

/// Returns the field types of `ty` when it is a struct that assigns bitwise.
///
/// Classes are excluded because they carry reference semantics by design, and a
/// struct with a managed field is excluded because it is reference counted, so
/// two names for it share one object on purpose.
fn value_aggregate_fields(ctx: &LoweringContext, ty: &Type) -> Option<Vec<Type>> {
    let TypeKind::Custom(name, _) = &ty.kind else {
        return None;
    };
    if !ctx.is_type_auto_copy(ty) {
        return None;
    }
    let definitions = ctx.type_checker.type_definitions();
    let crate::type_checker::context::TypeDefinition::Struct(definition) =
        definitions.get(name.as_str())?
    else {
        return None;
    };
    Some(
        definition
            .fields
            .iter()
            .map(|(_, field_ty, _)| field_ty.clone())
            .collect(),
    )
}
