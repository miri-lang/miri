// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering for the list methods that hand an element back: `pop` and
//! `remove_at`, which remove the element they return, and `first` and `last`,
//! which leave it where it is.
//!
//! Each is written once in the standard library over an opaque element type,
//! and reads one value word out of the slot — for an inline element the first
//! eight bytes of its components rather than the element. Even read at its true
//! stride the element would only be an address into storage the list is about to
//! shrink and later free, so the caller would end up owning a pointer into a
//! buffer that is not its own.
//!
//! Lowering the call here is what makes the element type concrete: the
//! components are read at their real offsets and copied into a value of their
//! own before the list gives up the slot they came from.
//!
//! A method that only reads needs the copy just as much. It answers with an
//! optional, and an optional releases a vector payload, so handing back the
//! address of the element inside the list would have the optional free a
//! pointer into a buffer it does not own.

use crate::ast::expression::Expression;
use crate::ast::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BinOp, Discriminant, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::rt;

use super::constructors::int_constant;
use super::helpers::coerce_rvalue_in;
use super::inline_element::{
    binary_into_bool, call_runtime, copy_inline_element, int, none, store_temp, InlineElement,
};
use super::{lower_expression, LoweringContext};

/// Which element a call reads out of the list, and whether it removes it.
pub(super) enum InlineElementRead<'a> {
    /// `pop()` — the element at the end, removed.
    Pop,
    /// `remove_at(index)` — the element at `index`, removed, shifting the rest
    /// down.
    RemoveAt(&'a Expression),
    /// `first()` — the element at the front, left in place.
    First,
    /// `last()` — the element at the end, left in place.
    Last,
}

impl<'a> InlineElementRead<'a> {
    /// The read `method_name` names, or `None` when the method does not hand an
    /// element back.
    pub(super) fn of(method_name: &str, args: &'a [Expression]) -> Option<Self> {
        match (method_name, args) {
            ("pop", []) => Some(Self::Pop),
            ("remove_at", [index]) => Some(Self::RemoveAt(index)),
            ("first", []) => Some(Self::First),
            ("last", []) => Some(Self::Last),
            _ => None,
        }
    }

    /// The runtime entry that drops the element out of the list, and the
    /// arguments it takes after the list itself; `None` for a read that leaves
    /// the element where it is.
    ///
    /// Neither entry releases the element: the caller is walking away with it.
    fn removal_call(&self, index: Local) -> Option<(&'static str, Vec<Operand>)> {
        match self {
            Self::Pop => Some((rt::LIST_POP, Vec::new())),
            Self::RemoveAt(_) => Some((rt::LIST_TAKE_AT, vec![Operand::Copy(Place::new(index))])),
            Self::First | Self::Last => None,
        }
    }
}

/// Lower `list.pop()`, `list.remove_at(i)`, `list.first()` or `list.last()` for
/// a list of inline elements.
///
/// The shape mirrors the standard library body: an out-of-range index answers
/// `None`, and any other copies the element out, removes it, and answers
/// `Some`. The copy is what the body cannot express, and it has to happen
/// before the removal — after it the slot is either past the end or holding a
/// later element shifted down over it.
pub(super) fn lower_inline_element_read(
    ctx: &mut LoweringContext,
    obj: &Expression,
    list_ty: &Type,
    element: &InlineElement,
    removal: InlineElementRead<'_>,
    span: &Span,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let obj_op = super::dispatch::move_to_copy(lower_expression(ctx, obj, None)?);
    let obj_src = match &obj_op {
        Operand::Copy(place) | Operand::Move(place) => Some(place.local),
        Operand::Constant(_) => None,
    };
    let list = store_temp(ctx, obj_op, list_ty.clone(), *span);
    let (len, index) = lower_length_and_index(ctx, list, &removal, *span)?;

    let option_ty = Type::new(TypeKind::Option(Box::new(element.ty.clone())), *span);
    let (result, op) = super::dispatch::call_destination(ctx, option_ty.clone(), dest, *span);
    let sites = TakeSites {
        list,
        index,
        len,
        result,
    };
    emit_take_or_absent(ctx, sites, element, &option_ty, &removal, *span);

    ctx.emit_temp_drop(list, watermark, *span);
    if let Some(src) = obj_src {
        ctx.emit_temp_drop(src, watermark, *span);
    }
    Ok(op)
}

/// Measure the list and resolve the index the removal names.
///
/// The length is read once and both the index and the range test are built from
/// that one reading, so they cannot disagree about how long the list is.
fn lower_length_and_index(
    ctx: &mut LoweringContext,
    list: Local,
    removal: &InlineElementRead<'_>,
    span: Span,
) -> Result<(Local, Local), LoweringError> {
    let len = call_runtime(
        ctx,
        rt::LIST_LEN,
        vec![Operand::Copy(Place::new(list))],
        int(span),
    );
    let index = match removal {
        InlineElementRead::Pop | InlineElementRead::Last => last_index(ctx, len, span),
        InlineElementRead::First => store_temp(ctx, int_constant(0, &span), int(span), span),
        InlineElementRead::RemoveAt(arg) => {
            let op = lower_expression(ctx, arg, None)?;
            store_temp(ctx, super::dispatch::move_to_copy(op), int(span), arg.span)
        }
    };
    Ok((len, index))
}

/// The locals a take works over: the list, the index into it, the length it was
/// measured against, and the optional the call answers with.
struct TakeSites {
    list: Local,
    index: Local,
    len: Local,
    result: Place,
}

/// Emit the branch that answers `Some(element)` for an index the list holds and
/// `None` for any other.
fn emit_take_or_absent(
    ctx: &mut LoweringContext,
    sites: TakeSites,
    element: &InlineElement,
    option_ty: &Type,
    removal: &InlineElementRead<'_>,
    span: Span,
) {
    let in_range = index_in_range(ctx, sites.index, sites.len, span);
    let take_bb = ctx.new_basic_block();
    let absent_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(in_range)),
            targets: vec![(Discriminant::from(1u128), take_bb)],
            otherwise: absent_bb,
        },
        span,
    ));

    ctx.set_current_block(take_bb);
    emit_take(ctx, &sites, element, option_ty, removal, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        span,
    ));

    ctx.set_current_block(absent_bb);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(sites.result.clone(), Rvalue::Use(none(option_ty, span))),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        span,
    ));

    ctx.set_current_block(join_bb);
}

/// Copy the element out, drop it from the list when the read removes it, and
/// store it as `Some`.
///
/// The copy comes first: once the element is removed its slot is either past the
/// end or holding the element that shifted down over it.
fn emit_take(
    ctx: &mut LoweringContext,
    sites: &TakeSites,
    element: &InlineElement,
    option_ty: &Type,
    removal: &InlineElementRead<'_>,
    span: Span,
) {
    let watermark = ctx.body.local_decls.len();
    let copied = copy_inline_element(ctx, sites.list, sites.index, element, span);
    if let Some((entry, args)) = removal.removal_call(sites.index) {
        let call_args = [vec![Operand::Copy(Place::new(sites.list))], args].concat();
        call_runtime(ctx, entry, call_args, Type::new(TypeKind::Boolean, span));
    }

    let wrapped = coerce_rvalue_in(
        ctx,
        Operand::Copy(Place::new(copied)),
        &element.ty,
        option_ty,
        span,
    );
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(sites.result.clone(), wrapped),
        span,
    });
    // Boxing the element as `Some` reads it by copy, which takes a second
    // reference; the temp holding the first one has no scope to release it.
    ctx.emit_temp_drop(copied, watermark, span);
}

/// `index >= 0 && index < len`, as a boolean local.
fn index_in_range(ctx: &mut LoweringContext, index: Local, len: Local, span: Span) -> Local {
    let non_negative = binary_into_bool(
        ctx,
        BinOp::Ge,
        Operand::Copy(Place::new(index)),
        int_constant(0, &span),
        span,
    );
    let below_len = binary_into_bool(
        ctx,
        BinOp::Lt,
        Operand::Copy(Place::new(index)),
        Operand::Copy(Place::new(len)),
        span,
    );
    binary_into_bool(
        ctx,
        BinOp::BitAnd,
        Operand::Copy(Place::new(non_negative)),
        Operand::Copy(Place::new(below_len)),
        span,
    )
}

/// `len - 1`, the index `pop` removes; `-1` for an empty list, which
/// [`index_in_range`] then rejects.
fn last_index(ctx: &mut LoweringContext, len: Local, span: Span) -> Local {
    let index = ctx.push_temp(int(span), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(index),
            Rvalue::BinaryOp(
                BinOp::Sub,
                Box::new(Operand::Copy(Place::new(len))),
                Box::new(int_constant(1, &span)),
            ),
        ),
        span,
    });
    index
}
