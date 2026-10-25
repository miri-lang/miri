// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! In-place appends to a string: `s = s + t + …` and `s += t` on a string
//! local, and `obj.f = obj.f + t + …` and `obj.f += t` on a `String` field.
//!
//! The runtime's append takes over the reference it is handed and extends the
//! string in place when nothing else holds it. A local hands its own reference
//! over directly. A field cannot: the object keeps holding its value while the
//! append runs. So the field's value is taken into a temp, the field is
//! emptied — which drops the object's reference and leaves the temp the only
//! holder unless another binding shares the value — the temp is appended to,
//! and the result is stored back. A value something else still holds is
//! copied by the append, so every other holder keeps the text it had.

use super::assignment_expr::{
    field_index, field_slot_type, finalize_assign_result, resolve_member_field_index,
};
use super::unshare::{lower_written_entry_place, path_root};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::operator::{AssignmentOp, BinaryOp};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_string_append, lower_expression};
use crate::mir::lowering::helpers::ensure_place;
use crate::mir::{Constant, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind};

/// What `x = x + t + u` or `x += t` appends to the string `x` names.
pub(super) struct Appended<'e> {
    /// The expression the `=` form reads the target's old value through, the
    /// leftmost operand of its sum; `None` for `+=`.
    pub old: Option<&'e Expression>,
    /// The strings appended, in order.
    pub pieces: Vec<&'e Expression>,
}

/// The strings `x = x + t + u` or `x += t` appends, in order, to its target,
/// where `reaches_target` tells whether an expression names that target: the
/// `=` form qualifies only when the leftmost operand of its sum does. `None`
/// unless every piece is a string.
pub(super) fn appended_pieces<'e>(
    ctx: &LoweringContext,
    op: &AssignmentOp,
    rhs: &'e Expression,
    reaches_target: impl Fn(&Expression) -> bool,
) -> Option<Appended<'e>> {
    let appended = match op {
        AssignmentOp::AssignAdd => Appended {
            old: None,
            pieces: vec![rhs],
        },
        AssignmentOp::Assign => summed_onto_target(rhs, reaches_target)?,
        AssignmentOp::AssignSub
        | AssignmentOp::AssignMul
        | AssignmentOp::AssignDiv
        | AssignmentOp::AssignMod => return None,
    };
    let is_string = |piece: &&Expression| {
        ctx.recorded_type(piece.id)
            .is_some_and(|ty| ty.kind == TypeKind::String)
    };
    appended.pieces.iter().all(is_string).then_some(appended)
}

/// Appends the string `appended` holds onto the string `target` holds: the
/// append takes over `target`'s reference and the result is written back into
/// it.
pub(super) fn append_in_place(
    ctx: &mut LoweringContext,
    target: Local,
    appended: Local,
    span: Span,
) {
    let result = emit_string_append(ctx, target, appended, &span);
    // `Assign`, not `Reassign`: the call took over what `target` held.
    push_statement(
        ctx,
        StatementKind::Assign(
            Place::new(target),
            Rvalue::Use(Operand::Move(Place::new(result))),
        ),
        span,
    );
}

/// The local holding the string `place` reaches: the local itself when
/// `place` is a whole local, else a temp the string is copied into, holding a
/// reference of its own for the caller to release.
pub(super) fn whole_local(ctx: &mut LoweringContext, place: &Place, span: Span) -> Local {
    if place.projection.is_empty() {
        place.local
    } else {
        copy_into_temp(ctx, place, span)
    }
}

/// The field and the pieces of an assignment to `member` that appends onto a
/// `String` field in place: a host body and a path not starting at a
/// GPU-resident binding, a field the instance holds at `String` — a generic
/// field instantiated at `String` included, an optional one not — and an `=`
/// form whose sum starts at the very same field.
pub(super) fn field_append_target<'e>(
    ctx: &LoweringContext,
    member: &'e Expression,
    op: &AssignmentOp,
    rhs: &'e Expression,
) -> Option<(FieldAppend<'e>, Appended<'e>)> {
    let ExpressionKind::Member(obj, prop) = &member.node else {
        return None;
    };
    if !is_host_path(ctx, obj) {
        return None;
    }
    let obj_ty = ctx.recorded_type(obj.id)?;
    let TypeKind::Custom(type_name, _) = &obj_ty.kind else {
        return None;
    };
    let idx = resolve_member_field_index(type_name, prop, ctx.type_checker.type_definitions())?;
    let slot = field_slot_type(ctx, &obj_ty, type_name, idx)?;
    if slot.kind != TypeKind::String {
        return None;
    }
    let reaches_target = |candidate: &Expression| same_path(ctx, member, candidate);
    let appended = appended_pieces(ctx, op, rhs, reaches_target)?;
    Some((FieldAppend { obj, idx }, appended))
}

/// Whether the path `obj` names lives on the host: the body is not a GPU
/// body, and the binding the path starts at, if any, mirrors no device buffer.
fn is_host_path(ctx: &LoweringContext, obj: &Expression) -> bool {
    let mirrors_device = bound_local(ctx, path_root(obj))
        .and_then(|local| ctx.body.local_decls.get(local.0))
        .is_some_and(|decl| decl.device_handle.is_some());
    !ctx.body.is_gpu() && !mirrors_device
}

/// Whether `a` and `b` name the same place without either being evaluated:
/// the same binding, the same field of the same place, or the same element of
/// the same collection under an index that is the same binding or the same
/// literal. Anything else — a call, an index computed by an expression — is
/// never the same, since reading it twice could give two places.
///
/// This decides only whether an assignment qualifies for the in-place append,
/// never whether two places are one: the `=` form still lowers its leftmost
/// operand as an expression of its own, so a wrong answer costs the append's
/// speed, not its correctness. Do not use it as proof that two places alias.
pub(super) fn same_path(ctx: &LoweringContext, a: &Expression, b: &Expression) -> bool {
    if let (ExpressionKind::Member(a_obj, a_prop), ExpressionKind::Member(b_obj, b_prop)) =
        (&a.node, &b.node)
    {
        let a_idx = member_field_index(ctx, a_obj, a_prop);
        return a_idx.is_some()
            && a_idx == member_field_index(ctx, b_obj, b_prop)
            && same_path(ctx, a_obj, b_obj);
    }
    if let (ExpressionKind::Index(a_list, a_index), ExpressionKind::Index(b_list, b_index)) =
        (&a.node, &b.node)
    {
        return same_index(ctx, a_index, b_index) && same_path(ctx, a_list, b_list);
    }
    same_binding(ctx, a, b)
}

/// The pieces of `rhs` when it is a sum whose leftmost operand
/// `reaches_target`: `x + a + b` is `(x + a) + b`, so the walk goes down the
/// left operands to `x`.
fn summed_onto_target(
    rhs: &Expression,
    reaches_target: impl Fn(&Expression) -> bool,
) -> Option<Appended<'_>> {
    let mut pieces = Vec::new();
    let mut current = rhs;
    while let ExpressionKind::Binary(left, BinaryOp::Add, right) = &current.node {
        pieces.push(right.as_ref());
        current = left.as_ref();
    }
    if pieces.is_empty() || !reaches_target(current) {
        return None;
    }
    pieces.reverse();
    Some(Appended {
        old: Some(current),
        pieces,
    })
}

/// Whether `a` and `b` are identifiers resolving to one local.
fn same_binding(ctx: &LoweringContext, a: &Expression, b: &Expression) -> bool {
    bound_local(ctx, a).is_some_and(|local| bound_local(ctx, b) == Some(local))
}

/// The local the identifier `expr` names.
fn bound_local(ctx: &LoweringContext, expr: &Expression) -> Option<Local> {
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        return None;
    };
    ctx.variable_map.get(name.as_str()).copied()
}

/// Whether two index operands are the same binding or the same literal.
fn same_index(ctx: &LoweringContext, a: &Expression, b: &Expression) -> bool {
    if let (ExpressionKind::Literal(a), ExpressionKind::Literal(b)) = (&a.node, &b.node) {
        return a == b;
    }
    same_binding(ctx, a, b)
}

/// The position of the field `obj.prop` names in the layout of `obj`'s type.
fn member_field_index(ctx: &LoweringContext, obj: &Expression, prop: &Expression) -> Option<usize> {
    field_index(ctx, &ctx.recorded_type(obj.id)?, prop)
}

/// The field a field append writes: the object expression and the field's
/// position in its layout.
pub(super) struct FieldAppend<'e> {
    pub obj: &'e Expression,
    pub idx: usize,
}

/// Lowers an append onto the `String` field `target` names: the old value is
/// taken where the assignment reads it, every piece is computed, the path to
/// the field is made the write's own, and the field is swapped out, appended
/// to and stored back. The expression's value is a reference of its own to
/// what the field holds afterwards.
pub(super) fn lower_field_append(
    ctx: &mut LoweringContext,
    target: FieldAppend<'_>,
    appended: &Appended<'_>,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let span = expr.span;
    let watermark = ctx.body.local_decls.len();
    let early = appended
        .old
        .map(|old| lower_owned_string(ctx, old, span))
        .transpose()?;
    let pieces = lower_pieces(ctx, &appended.pieces, span)?;
    let written = lower_written_entry_place(ctx, target.obj)?;
    let mut field = match &written {
        Some(written) => written.place.clone(),
        None => {
            let base = lower_expression(ctx, target.obj, None)?;
            ensure_place(ctx, base, target.obj.span)
        }
    };
    field.projection.push(PlaceElem::Field(target.idx));
    let held = early.map_or_else(|| copy_into_temp(ctx, &field, span), |old| old.local);
    swap_and_append(ctx, &field, held, &pieces, span);
    for piece in pieces {
        ctx.emit_temp_drop(piece.local, piece.watermark, span);
    }
    let result = copy_into_temp(ctx, &field, span);
    if let Some(written) = written {
        written.finish(ctx, target.obj);
    }
    let value = Operand::Copy(Place::new(result));
    finalize_assign_result(ctx, value, dest, expr, watermark)
}

/// A string lowered into a temp only its user reads, with the watermark that
/// tells whether the temp is one to release.
struct OwnedString {
    local: Local,
    watermark: usize,
}

/// Lowers every piece, left to right, before anything is appended, each into
/// a temp of its own: a piece that reads the field itself — or a binding a
/// later piece replaces — keeps the value it read once the field is emptied.
fn lower_pieces(
    ctx: &mut LoweringContext,
    pieces: &[&Expression],
    span: Span,
) -> Result<Vec<OwnedString>, LoweringError> {
    let mut lowered = Vec::with_capacity(pieces.len());
    for piece in pieces {
        lowered.push(lower_owned_string(ctx, piece, span)?);
    }
    Ok(lowered)
}

/// Lowers the string `expr` evaluates to into a temp holding a reference of
/// its own: the temp lowering produced when the expression owns it whole,
/// else a copy of what lowering produced, whose source is released.
fn lower_owned_string(
    ctx: &mut LoweringContext,
    expr: &Expression,
    span: Span,
) -> Result<OwnedString, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let lowered = lower_expression(ctx, expr, None)?;
    let place = ensure_place(ctx, lowered, expr.span);
    if is_fresh_temp(ctx, &place, watermark) {
        return Ok(OwnedString {
            local: place.local,
            watermark,
        });
    }
    let local = copy_into_temp(ctx, &place, span);
    ctx.emit_temp_drop(place.local, watermark, span);
    Ok(OwnedString { local, watermark })
}

/// Whether `place` is a whole temp lowering made after `watermark` that the
/// expression owns, which nothing but the expression reads.
fn is_fresh_temp(ctx: &LoweringContext, place: &Place, watermark: usize) -> bool {
    place.projection.is_empty()
        && ctx.is_expression_temp(place.local, watermark)
        && !ctx.is_owned_by_a_scope(place.local)
}

/// Copies the string `place` holds into a new temp, which takes a reference
/// of its own.
fn copy_into_temp(ctx: &mut LoweringContext, place: &Place, span: Span) -> Local {
    let temp = ctx.push_temp(Type::new(TypeKind::String, span), span);
    push_statement(
        ctx,
        StatementKind::Assign(Place::new(temp), Rvalue::Use(Operand::Copy(place.clone()))),
        span,
    );
    temp
}

/// Empties `field`, appends every piece onto `held` — its old value — and
/// stores the result back into `field`.
fn swap_and_append(
    ctx: &mut LoweringContext,
    field: &Place,
    held: Local,
    pieces: &[OwnedString],
    span: Span,
) {
    // The empty literal is immortal: storing it releases the field's
    // reference to the old value, and the store back releases nothing real.
    let empty = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::String, span),
        literal: Literal::String(String::new()),
    }));
    push_statement(
        ctx,
        StatementKind::Reassign(field.clone(), Rvalue::Use(empty)),
        span,
    );
    for piece in pieces {
        append_in_place(ctx, held, piece.local, span);
    }
    push_statement(
        ctx,
        StatementKind::Reassign(field.clone(), Rvalue::Use(Operand::Move(Place::new(held)))),
        span,
    );
}

fn push_statement(ctx: &mut LoweringContext, kind: StatementKind, span: Span) {
    ctx.push_statement(Statement { kind, span });
}
