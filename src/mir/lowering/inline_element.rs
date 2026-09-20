// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a list element laid out inline is, how a call site recognizes one, and
//! how to read one out at its real width.
//!
//! A standard library body reaches an element over an opaque element type and
//! reads one value word out of the slot. For an element the list lays out
//! inline that word is the first eight bytes of its components rather than the
//! element, so every method written that way is wrong for such a list — some
//! by crashing, some by answering as if the element were absent.
//!
//! A call site is where the element type is concrete, so it is where the
//! components can be addressed at their real offsets. The pieces here are what
//! the call-site lowerings that read an element are built from.

use crate::ast::{types, Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::{
    AggregateKind, BinOp, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};

use super::LoweringContext;

/// An element type a list lays out inline, and how many components it carries.
///
/// Both come from the one decision that recognizes the layout, so a copy can
/// never read a different number of components than the layout was matched on.
pub(super) struct InlineElement {
    pub(super) ty: Type,
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

/// Read every component of the element at `index` and build a value of its own
/// out of them.
///
/// The components are read through the index projection, which is what addresses
/// them at the element's stride and at their own width; assembling them into an
/// aggregate is what gives the caller storage the list does not own.
pub(super) fn copy_inline_element(
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

/// Store `op` into a fresh temp of `ty` and return the temp.
pub(super) fn store_temp(ctx: &mut LoweringContext, op: Operand, ty: Type, span: Span) -> Local {
    let local = ctx.push_temp(ty, span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(local), Rvalue::Use(op)),
        span,
    });
    local
}

/// Evaluate `lhs op rhs` into a fresh boolean temp and return it.
pub(super) fn binary_into_bool(
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
pub(super) fn call_runtime(
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
pub(super) fn none(option_ty: &Type, span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: option_ty.clone(),
        literal: crate::ast::literal::Literal::None,
    }))
}

/// The `int` type at `span`.
pub(super) fn int(span: Span) -> Type {
    Type::new(TypeKind::Int, span)
}
