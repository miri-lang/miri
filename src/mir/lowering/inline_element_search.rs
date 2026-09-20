// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering for the list methods that look an element up by value: `contains`,
//! `index_of`, and `remove`, which removes the element `index_of` finds.
//!
//! Each is written once in the standard library over an opaque element type,
//! and reads one value word out of the slot to compare it. For an element the
//! list lays out inline that word is a prefix of the components, so the search
//! compares prefixes: it reports a present element absent, and would report two
//! elements the same when only their leading components agree. Neither failure
//! announces itself — the call answers, it just answers wrongly.
//!
//! Lowering the call here is what makes the element type concrete. The
//! comparison is then the same structural equality `==` emits for two of these
//! elements written out by hand, so a search cannot disagree with the operator
//! it is defined in terms of. Only the walk over the slots is built here.

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
use super::expression::structural_equality::emit_structural_equality;
use super::helpers::coerce_rvalue_in;
use super::inline_element::{
    binary_into_bool, call_runtime, copy_inline_element, int, none, store_temp, InlineElement,
};
use super::{lower_expression, LoweringContext};

/// Which answer a call wants out of the search, and the element it searches for.
pub(super) enum InlineElementSearch<'a> {
    /// `contains(item)` — whether the list holds it.
    Contains(&'a Expression),
    /// `index_of(item)` — the first slot holding it, or `None`.
    IndexOf(&'a Expression),
    /// `remove(item)` — take the first one that matches out of the list, and
    /// report whether there was one.
    Remove(&'a Expression),
}

impl<'a> InlineElementSearch<'a> {
    /// The search `method_name` names, or `None` when the method does not look
    /// an element up by value.
    pub(super) fn of(method_name: &str, args: &'a [Expression]) -> Option<Self> {
        match (method_name, args) {
            ("contains", [item]) => Some(Self::Contains(item)),
            ("index_of", [item]) => Some(Self::IndexOf(item)),
            ("remove", [item]) => Some(Self::Remove(item)),
            _ => None,
        }
    }

    /// The element being searched for.
    fn needle(&self) -> &'a Expression {
        match self {
            Self::Contains(item) | Self::IndexOf(item) | Self::Remove(item) => item,
        }
    }

    /// The type the call answers with.
    fn answer_ty(&self, span: Span) -> Type {
        match self {
            Self::Contains(_) | Self::Remove(_) => Type::new(TypeKind::Boolean, span),
            Self::IndexOf(_) => Type::new(TypeKind::Option(Box::new(int(span))), span),
        }
    }
}

/// Lower `list.contains(item)`, `list.index_of(item)` or `list.remove(item)`
/// for a list of inline elements.
///
/// The walk stops at the first slot that matches, which is what makes
/// `index_of` answer the first occurrence and `remove` take the first one.
pub(super) fn lower_inline_element_search(
    ctx: &mut LoweringContext,
    obj: &Expression,
    list_ty: &Type,
    element: &InlineElement,
    search: InlineElementSearch<'_>,
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

    let needle_op = super::dispatch::move_to_copy(lower_expression(ctx, search.needle(), None)?);
    let needle_src = match &needle_op {
        Operand::Copy(place) | Operand::Move(place) => Some(place.local),
        Operand::Constant(_) => None,
    };
    let needle = store_temp(ctx, needle_op, element.ty.clone(), *span);

    let answer_ty = search.answer_ty(*span);
    let (result, op) = super::dispatch::call_destination(ctx, answer_ty.clone(), dest, *span);

    let found = emit_first_matching_index(ctx, list, needle, element, *span)?;
    emit_answer(
        ctx,
        &search,
        &answer_ty,
        Sites {
            list,
            found,
            result,
        },
        *span,
    );

    // The search answers with an index, never with the element, so everything
    // it read stays here: the list it walked and the element it compared
    // against, which the call site built for this call alone.
    ctx.emit_temp_drop(list, watermark, *span);
    ctx.emit_temp_drop(needle, watermark, *span);
    for src in [obj_src, needle_src].into_iter().flatten() {
        ctx.emit_temp_drop(src, watermark, *span);
    }
    Ok(op)
}

/// The locals an answer is built from: the list searched, the index the search
/// settled on, and the place the call answers into.
struct Sites {
    list: Local,
    found: Local,
    result: Place,
}

/// Walk the list from the front and leave the index of the first element equal
/// to `needle` in a local, or `-1` when no element is.
///
/// The length is read once, before the walk: the search does not change it, and
/// reading it per slot would only cost a call.
fn emit_first_matching_index(
    ctx: &mut LoweringContext,
    list: Local,
    needle: Local,
    element: &InlineElement,
    span: Span,
) -> Result<Local, LoweringError> {
    let len = call_runtime(
        ctx,
        rt::LIST_LEN,
        vec![Operand::Copy(Place::new(list))],
        int(span),
    );
    let walk = Walk {
        slot: SlotCompare {
            list,
            index: store_temp(ctx, int_constant(0, &span), int(span), span),
            needle,
            element,
        },
        len,
        found: store_temp(ctx, int_constant(-1, &span), int(span), span),
    };
    let found = walk.found;
    emit_walk(ctx, walk, span)?;
    Ok(found)
}

/// One walk over the slots: the slot being compared, how many there are, and
/// where the index it settles on is recorded.
struct Walk<'a> {
    slot: SlotCompare<'a>,
    len: Local,
    found: Local,
}

/// Emit the blocks the walk is made of: the test that a slot is left, the
/// comparison of the slot the index names, and the step to the next one.
///
/// Leaves the current block at the one the walk ends in, whether it ran out of
/// slots or found a match.
fn emit_walk(ctx: &mut LoweringContext, walk: Walk<'_>, span: Span) -> Result<(), LoweringError> {
    let head_bb = ctx.new_basic_block();
    let compare_bb = ctx.new_basic_block();
    let step_bb = ctx.new_basic_block();
    let done_bb = ctx.new_basic_block();
    let index = walk.slot.index;
    goto(ctx, head_bb, span);

    ctx.set_current_block(head_bb);
    let more = binary_into_bool(
        ctx,
        BinOp::Lt,
        Operand::Copy(Place::new(index)),
        Operand::Copy(Place::new(walk.len)),
        span,
    );
    switch_on_bool(ctx, more, compare_bb, done_bb, span);

    ctx.set_current_block(compare_bb);
    let targets = MatchTargets {
        found: walk.found,
        step_bb,
        done_bb,
    };
    emit_compare_slot(ctx, walk.slot, targets, span)?;

    ctx.set_current_block(step_bb);
    emit_index_increment(ctx, index, span);
    goto(ctx, head_bb, span);

    ctx.set_current_block(done_bb);
    Ok(())
}

/// Advance `index` to the next slot.
fn emit_index_increment(ctx: &mut LoweringContext, index: Local, span: Span) {
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(index),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(index))),
                Box::new(int_constant(1, &span)),
            ),
        ),
        span,
    });
}

/// One slot's comparison: where the element is read from, and what it is
/// compared against.
struct SlotCompare<'a> {
    list: Local,
    index: Local,
    needle: Local,
    element: &'a InlineElement,
}

/// Where the comparison of one slot goes: it records the index it matched at,
/// or moves on to the next slot.
struct MatchTargets {
    found: Local,
    step_bb: crate::mir::BasicBlock,
    done_bb: crate::mir::BasicBlock,
}

/// Compare the element at `index` with the needle, recording the index and
/// leaving the walk when they are equal.
fn emit_compare_slot(
    ctx: &mut LoweringContext,
    slot: SlotCompare<'_>,
    targets: MatchTargets,
    span: Span,
) -> Result<(), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let copied = copy_inline_element(ctx, slot.list, slot.index, slot.element, span);
    // The element is read into a value of its own rather than compared in
    // place: structural equality projects fields out of what it is handed, and
    // a place inside the list already carries the index projection those would
    // be read through.
    let equal = emit_structural_equality(
        ctx,
        span,
        &slot.element.ty.kind,
        Operand::Copy(Place::new(copied)),
        Operand::Copy(Place::new(slot.needle)),
        true,
    )?;
    // The copy is per slot and the walk comes back round to make another, so it
    // is released here rather than left to a scope that only ends once.
    ctx.emit_temp_drop(copied, watermark, span);

    let hit_bb = ctx.new_basic_block();
    switch_on_bool(ctx, equal, hit_bb, targets.step_bb, span);

    ctx.set_current_block(hit_bb);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(targets.found),
            Rvalue::Use(Operand::Copy(Place::new(slot.index))),
        ),
        span,
    });
    goto(ctx, targets.done_bb, span);
    Ok(())
}

/// Turn the index the search settled on into what the call answers with.
fn emit_answer(
    ctx: &mut LoweringContext,
    search: &InlineElementSearch<'_>,
    answer_ty: &Type,
    sites: Sites,
    span: Span,
) {
    let matched = binary_into_bool(
        ctx,
        BinOp::Ge,
        Operand::Copy(Place::new(sites.found)),
        int_constant(0, &span),
        span,
    );
    match search {
        InlineElementSearch::Contains(_) => ctx.push_statement(Statement {
            kind: StatementKind::Assign(
                sites.result,
                Rvalue::Use(Operand::Copy(Place::new(matched))),
            ),
            span,
        }),
        InlineElementSearch::IndexOf(_) => {
            emit_optional_index(ctx, sites, answer_ty, matched, span)
        }
        InlineElementSearch::Remove(_) => emit_removal(ctx, sites, matched, span),
    }
}

/// Answer `Some(index)` for a slot the search matched and `None` otherwise.
fn emit_optional_index(
    ctx: &mut LoweringContext,
    sites: Sites,
    answer_ty: &Type,
    matched: Local,
    span: Span,
) {
    let some_bb = ctx.new_basic_block();
    let none_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();
    switch_on_bool(ctx, matched, some_bb, none_bb, span);

    ctx.set_current_block(some_bb);
    let wrapped = coerce_rvalue_in(
        ctx,
        Operand::Copy(Place::new(sites.found)),
        &int(span),
        answer_ty,
        span,
    );
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(sites.result.clone(), wrapped),
        span,
    });
    goto(ctx, join_bb, span);

    ctx.set_current_block(none_bb);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(sites.result, Rvalue::Use(none(answer_ty, span))),
        span,
    });
    goto(ctx, join_bb, span);

    ctx.set_current_block(join_bb);
}

/// Take the matched element out of the list, and answer whether there was one.
///
/// The removal is guarded by the match: the runtime entry takes an index, and
/// the index of no element is not one it can be asked for.
fn emit_removal(ctx: &mut LoweringContext, sites: Sites, matched: Local, span: Span) {
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            sites.result,
            Rvalue::Use(Operand::Copy(Place::new(matched))),
        ),
        span,
    });

    let remove_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();
    switch_on_bool(ctx, matched, remove_bb, join_bb, span);

    ctx.set_current_block(remove_bb);
    call_runtime(
        ctx,
        rt::LIST_REMOVE,
        vec![
            Operand::Copy(Place::new(sites.list)),
            Operand::Copy(Place::new(sites.found)),
        ],
        Type::new(TypeKind::Boolean, span),
    );
    goto(ctx, join_bb, span);

    ctx.set_current_block(join_bb);
}

/// Branch to `if_true` when `condition` holds and to `if_false` otherwise.
fn switch_on_bool(
    ctx: &mut LoweringContext,
    condition: Local,
    if_true: crate::mir::BasicBlock,
    if_false: crate::mir::BasicBlock,
    span: Span,
) {
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(condition)),
            targets: vec![(Discriminant::from(1u128), if_true)],
            otherwise: if_false,
        },
        span,
    ));
}

/// End the current block by jumping to `target`.
fn goto(ctx: &mut LoweringContext, target: crate::mir::BasicBlock, span: Span) {
    ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, span));
}
