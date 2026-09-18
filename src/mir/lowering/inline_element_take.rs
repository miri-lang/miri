// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering for the two list methods that hand an element back as they remove
//! it: `pop` and `remove_at`.
//!
//! Both are written once in the standard library over an opaque element type,
//! and that body is compiled once for every instantiation. An element the list
//! lays out inline has neither the width nor the representation that body
//! assumes: it reads one value word out of the slot, which for a vector is the
//! first eight bytes of its components, and hands that word back as if it were
//! the value. Even read at its true stride the element would only be an address
//! into storage the list is about to shrink and later free, so the caller would
//! end up owning a pointer into a buffer that is not its own.
//!
//! Lowering the call here is what makes the element type concrete: the
//! components are read at their real offsets and copied into a value of their
//! own before the list gives up the slot they came from.

use crate::ast::expression::Expression;
use crate::ast::{types, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    AggregateKind, BinOp, Discriminant, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;

use super::constructors::int_constant;
use super::helpers::coerce_rvalue_in;
use super::{lower_expression, LoweringContext};

/// Which element a call takes out of the list.
pub(super) enum ListRemoval<'a> {
    /// `pop()` — the element at the end.
    Last,
    /// `remove_at(index)` — the element at `index`, shifting the rest down.
    At(&'a Expression),
}

impl<'a> ListRemoval<'a> {
    /// The removal `method_name` names, or `None` when the method removes
    /// nothing or does not hand the element back.
    ///
    /// TODO: `first`, `last`, `contains` and `index_of` reach an element through
    /// the same generic body and are wrong for an inline element in the same
    /// way — the first two crash, the other two compare a prefix of the
    /// components and answer that a present vector is absent. They need the
    /// element type made concrete here too.
    pub(super) fn of(method_name: &str, args: &'a [Expression]) -> Option<Self> {
        match (method_name, args) {
            ("pop", []) => Some(Self::Last),
            ("remove_at", [index]) => Some(Self::At(index)),
            _ => None,
        }
    }

    /// The runtime entry that drops the element out of the list, and the
    /// arguments it takes after the list itself.
    ///
    /// Neither entry releases the element: the caller is walking away with it.
    fn runtime_call(&self, index: Local) -> (&'static str, Vec<Operand>) {
        match self {
            Self::Last => (rt::LIST_POP, Vec::new()),
            Self::At(_) => (rt::LIST_TAKE_AT, vec![Operand::Copy(Place::new(index))]),
        }
    }
}

/// An element type a list lays out inline, and how many components it carries.
///
/// Both come from the one decision that recognizes the layout, so the copy can
/// never read a different number of components than the layout was matched on.
pub(super) struct InlineElement {
    ty: Type,
    components: usize,
}

/// The inline element type of `list_ty`, or `None` when its elements travel as
/// value words and the standard library body is already correct.
pub(super) fn inline_element(ctx: &LoweringContext, list_ty: &Type) -> Option<InlineElement> {
    let TypeKind::Custom(_, args) = &list_ty.kind else {
        return None;
    };
    let ty = ctx.resolved_type(args.as_ref()?.first()?);
    types::inline_element_layout(&ty.kind)?;
    // Only a vector has an inline layout, and its dimension is its field count.
    let components = types::vec_type_dim(&ty.kind)?.into();
    Some(InlineElement { ty, components })
}

/// Lower `list.pop()` / `list.remove_at(i)` for a list of inline elements.
///
/// The shape mirrors the standard library body: an out-of-range index answers
/// `None`, and any other copies the element out, removes it, and answers
/// `Some`. The copy is what the body cannot express, and it has to happen
/// before the removal — after it the slot is either past the end or holding a
/// later element shifted down over it.
pub(super) fn lower_inline_element_take(
    ctx: &mut LoweringContext,
    obj: &Expression,
    list_ty: &Type,
    element: &InlineElement,
    removal: ListRemoval<'_>,
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
    removal: &ListRemoval<'_>,
    span: Span,
) -> Result<(Local, Local), LoweringError> {
    let len = call_runtime(
        ctx,
        rt::LIST_LEN,
        vec![Operand::Copy(Place::new(list))],
        int(span),
    );
    let index = match removal {
        ListRemoval::Last => last_index(ctx, len, span),
        ListRemoval::At(arg) => {
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
    removal: &ListRemoval<'_>,
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

/// Copy the element out, drop it from the list, and store it as `Some`.
///
/// The copy comes first: once the element is removed its slot is either past the
/// end or holding the element that shifted down over it.
fn emit_take(
    ctx: &mut LoweringContext,
    sites: &TakeSites,
    element: &InlineElement,
    option_ty: &Type,
    removal: &ListRemoval<'_>,
    span: Span,
) {
    let watermark = ctx.body.local_decls.len();
    let copied = copy_inline_element(ctx, sites.list, sites.index, element, span);
    let (entry, args) = removal.runtime_call(sites.index);
    let call_args = [vec![Operand::Copy(Place::new(sites.list))], args].concat();
    call_runtime(ctx, entry, call_args, Type::new(TypeKind::Boolean, span));

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

/// Read every component of the element at `index` and build a value of its own
/// out of them.
///
/// The components are read through the index projection, which is what addresses
/// them at the element's stride and at their own width; assembling them into an
/// aggregate is what gives the caller storage the list does not own.
fn copy_inline_element(
    ctx: &mut LoweringContext,
    list: Local,
    index: Local,
    element: &InlineElement,
    span: Span,
) -> Local {
    let components = (0..element.components)
        .map(|field| {
            Operand::Copy(Place {
                local: list,
                projection: vec![PlaceElem::Index(index), PlaceElem::Field(field)],
            })
        })
        .collect();
    let copied = ctx.push_temp(element.ty.clone(), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(copied),
            Rvalue::Aggregate(AggregateKind::Struct(element.ty.clone()), components),
        ),
        span,
    });
    copied
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

/// Store `op` into a fresh temp of `ty` and return the temp.
fn store_temp(ctx: &mut LoweringContext, op: Operand, ty: Type, span: Span) -> Local {
    let local = ctx.push_temp(ty, span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(local), Rvalue::Use(op)),
        span,
    });
    local
}

/// Evaluate `lhs op rhs` into a fresh boolean temp and return it.
fn binary_into_bool(
    ctx: &mut LoweringContext,
    op: BinOp,
    lhs: Operand,
    rhs: Operand,
    span: Span,
) -> Local {
    let local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::BinaryOp(op, Box::new(lhs), Box::new(rhs)),
        ),
        span,
    });
    local
}

/// Call a runtime entry, returning the temp its result lands in.
fn call_runtime(
    ctx: &mut LoweringContext,
    name: &str,
    args: Vec<Operand>,
    return_ty: Type,
) -> Local {
    let span = return_ty.span;
    let destination = ctx.push_temp(return_ty, span);
    let target = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: super::dispatch::runtime_fn_operand(name, span),
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(destination),
            target: Some(target),
        },
        span,
    ));
    ctx.set_current_block(target);
    destination
}

/// The absent optional of type `option_ty`.
fn none(option_ty: &Type, span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: option_ty.clone(),
        literal: crate::ast::literal::Literal::None,
    }))
}

/// The `int` type at `span`.
fn int(span: Span) -> Type {
    Type::new(TypeKind::Int, span)
}
