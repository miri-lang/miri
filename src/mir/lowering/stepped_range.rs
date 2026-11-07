// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `for x in a..b` over a range whose values step through their own methods:
//! a one-character string, or a class implementing `Steppable`.
//!
//! The loop calls the element type's `compare`, `successor` and `steppable`
//! the way a written call on a value of that type does, so a subclass's
//! override answers for the values it holds. The integers, which the machine
//! steps by adding one, are lowered in `loops`.

use crate::ast::statement::Statement;
use crate::ast::types::{
    Type, TypeKind, ORDERING_METHOD_NAME, STEPPABLE_METHOD_NAME, SUCCESSOR_METHOD_NAME,
};
use crate::ast::{BinaryOp, Expression, VariableDeclaration};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{BasicBlock, BinOp, Local, Operand, Place, Rvalue, StatementKind};

use super::expression::binary_expr::zero_operand;
use super::expression::range_expr::written_inclusivity;
use super::loops::{
    assign, goto, lower_range_pass, range_value_parts, switch_on, EndTest, PassBlocks, WrittenRange,
};
use super::method_dispatch::{emit_held_receiver_method_call, HeldReceiverCall};
use super::{emit_diverging_intrinsic_and_seal, lower_expression, LoweringContext};

/// A `for` loop over a range whose values step through their own methods:
/// the loop variable, the local holding the end, how the end is tested, and
/// the element type whose `compare`, `successor` and `steppable` it calls.
struct SteppedLoop {
    var: Local,
    end: Local,
    end_test: EndTest,
    element: Type,
}

/// Lower `for x in start..end` over values of a type that steps through its
/// own `successor`, a string or a class implementing `Steppable`.
///
/// The bounds are evaluated once, the start into the loop variable and the
/// end into a temp the loop's scope owns, so `break` and `return` release
/// both. The loop variable is bound only after both are evaluated, so a
/// bound may read a name the variable shadows.
pub(super) fn lower_for_over_stepped_range(
    ctx: &mut LoweringContext,
    span: &Span,
    decl: &VariableDeclaration,
    range: WrittenRange,
    element: Type,
    body: &Statement,
) -> Result<(), LoweringError> {
    let inclusive = written_inclusivity(range.kind)
        .ok_or_else(|| LoweringError::unsupported_range_type(*span))?;
    ctx.push_scope();
    let var = ctx.alloc_local(decl.name.clone(), element.clone(), *span);
    assign_bound(ctx, var, range.start)?;
    let end = ctx.push_temp(element.clone(), *span);
    ctx.register_scope_temp(end);
    assign_bound(ctx, end, range.end)?;
    ctx.bind_local_name(decl.name.clone(), var);
    let stepped = SteppedLoop {
        var,
        end,
        end_test: EndTest::Written(inclusive),
        element,
    };
    lower_stepped_loop(ctx, &stepped, body, span)?;
    ctx.pop_scope(*span);
    Ok(())
}

/// Lower `for x in r` over a range value `r` of values of a type that steps
/// through its own `successor`.
///
/// The range is evaluated into a temp the loop's scope owns; its start is
/// copied into the loop variable, and its end and inclusive flag into locals
/// of their own, each read once before the first pass.
pub(super) fn lower_for_over_stepped_range_value(
    ctx: &mut LoweringContext,
    span: &Span,
    decl: &VariableDeclaration,
    range_value: &Expression,
    element: Type,
    body: &Statement,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    let parts = range_value_parts(ctx, range_value, span)?;
    let var = ctx.alloc_local(decl.name.clone(), element.clone(), *span);
    assign(ctx, var, Rvalue::Use(parts.start), span);
    let end = ctx.push_temp(element.clone(), *span);
    ctx.register_scope_temp(end);
    assign(ctx, end, Rvalue::Use(parts.end), span);
    let inclusive = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    assign(ctx, inclusive, Rvalue::Use(parts.inclusive), span);
    ctx.bind_local_name(decl.name.clone(), var);
    let stepped = SteppedLoop {
        var,
        end,
        end_test: EndTest::Flag(inclusive),
        element,
    };
    lower_stepped_loop(ctx, &stepped, body, span)?;
    ctx.pop_scope(*span);
    Ok(())
}

/// Evaluate `bound` into `local`, which then holds a reference of its own
/// the way a binding initialized with it does.
fn assign_bound(
    ctx: &mut LoweringContext,
    local: Local,
    bound: &Expression,
) -> Result<(), LoweringError> {
    lower_expression(ctx, bound, Some(Place::new(local)))?;
    Ok(())
}

/// Lower the passes of a loop stepping through its values' own methods.
///
/// Before the first pass the loop asks `steppable()` of the start and then
/// the end, and stops the program when either cannot be stepped. Each pass
/// runs while the variable sorts before the end, or at it when the end is
/// included, as the element type's `compare` answers. After a pass an
/// inclusive range leaves once the variable has reached its end, before
/// asking for a successor, so the end of the type's values is never stepped
/// past; otherwise the variable is replaced by its `successor()`.
fn lower_stepped_loop(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    body: &Statement,
    span: &Span,
) -> Result<(), LoweringError> {
    refuse_unsteppable_bounds(ctx, stepped, span)?;
    let header_bb = ctx.new_basic_block();
    let blocks = PassBlocks {
        body: ctx.new_basic_block(),
        step: ctx.new_basic_block(),
        exit: ctx.new_basic_block(),
    };
    goto(ctx, header_bb, span);

    ctx.set_current_block(header_bb);
    emit_stepped_header(ctx, stepped, (blocks.body, blocks.exit), span)?;
    lower_range_pass(ctx, body, &blocks, span)?;

    ctx.set_current_block(blocks.step);
    emit_stepped_latch(ctx, stepped, blocks.exit, span)?;
    emit_successor_step(ctx, stepped, span)?;
    goto(ctx, header_bb, span);
    ctx.set_current_block(blocks.exit);
    Ok(())
}

/// Ask `steppable()` of the loop's start and then its end, and stop the
/// program with MER_RT_019 at the first that answers `false`.
fn refuse_unsteppable_bounds(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    span: &Span,
) -> Result<(), LoweringError> {
    for bound in [stepped.var, stepped.end] {
        let boolean = Type::new(TypeKind::Boolean, *span);
        let answer =
            call_element_method(ctx, stepped, (STEPPABLE_METHOD_NAME, boolean), bound, span)?;
        let steppable = ctx.new_basic_block();
        let refused = ctx.new_basic_block();
        switch_on(
            ctx,
            Operand::Copy(Place::new(answer)),
            (steppable, refused),
            span,
        );
        ctx.set_current_block(refused);
        emit_diverging_intrinsic_and_seal(
            ctx,
            &crate::mir::compiler_intrinsics::RANGE_BOUND_NOT_STEPPABLE_PANIC,
            Vec::new(),
            *span,
        );
        ctx.set_current_block(steppable);
    }
    Ok(())
}

/// Run a pass when the variable has not passed the end: it sorts before the
/// end, or at it when the end is included.
fn emit_stepped_header(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    pass_or_exit: (BasicBlock, BasicBlock),
    span: &Span,
) -> Result<(), LoweringError> {
    let flag = match stepped.end_test {
        EndTest::Written(inclusive) => {
            let op = if inclusive {
                BinaryOp::LessThanEqual
            } else {
                BinaryOp::LessThan
            };
            return switch_on_order(ctx, stepped, op, pass_or_exit, span);
        }
        EndTest::Flag(flag) => Operand::Copy(Place::new(flag)),
    };
    let at_most = ctx.new_basic_block();
    let below = ctx.new_basic_block();
    switch_on(ctx, flag, (at_most, below), span);
    ctx.set_current_block(at_most);
    switch_on_order(ctx, stepped, BinaryOp::LessThanEqual, pass_or_exit, span)?;
    ctx.set_current_block(below);
    switch_on_order(ctx, stepped, BinaryOp::LessThan, pass_or_exit, span)
}

/// After a pass of an inclusive range, leave once the variable no longer
/// sorts before the end: it has run at the end, whose successor is not asked
/// for. An exclusive range never runs at its end, so it steps directly.
fn emit_stepped_latch(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    exit_bb: BasicBlock,
    span: &Span,
) -> Result<(), LoweringError> {
    let flag = match stepped.end_test {
        EndTest::Written(false) => return Ok(()),
        EndTest::Written(true) => None,
        EndTest::Flag(flag) => Some(flag),
    };
    let step_bb = ctx.new_basic_block();
    if let Some(flag) = flag {
        let at_end_check = ctx.new_basic_block();
        switch_on(
            ctx,
            Operand::Copy(Place::new(flag)),
            (at_end_check, step_bb),
            span,
        );
        ctx.set_current_block(at_end_check);
    }
    switch_on_order(ctx, stepped, BinaryOp::LessThan, (step_bb, exit_bb), span)?;
    ctx.set_current_block(step_bb);
    Ok(())
}

/// Go to `when_true` when `variable op end` holds by the variable's
/// `compare`, called as a written `variable.compare(end)` is, and to
/// `otherwise` when it does not.
fn switch_on_order(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    op: BinaryOp,
    targets: (BasicBlock, BasicBlock),
    span: &Span,
) -> Result<(), LoweringError> {
    let against_zero = match op {
        BinaryOp::LessThan => BinOp::Lt,
        BinaryOp::LessThanEqual => BinOp::Le,
        other => {
            return Err(LoweringError::internal(
                crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
                format!("a stepped loop tests its end with `<` or `<=`, not `{other:?}`"),
                *span,
            ))
        }
    };
    let int = Type::new(TypeKind::Int, *span);
    let order = ctx.push_temp(int, *span);
    let call = HeldReceiverCall {
        receiver_ty: &stepped.element,
        method_name: ORDERING_METHOD_NAME,
        args: vec![
            Operand::Copy(Place::new(stepped.var)),
            Operand::Copy(Place::new(stepped.end)),
        ],
    };
    emit_held_receiver_method_call(ctx, call, &Place::new(order), *span)?;
    let holds = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    let rvalue = Rvalue::BinaryOp(
        against_zero,
        Box::new(Operand::Copy(Place::new(order))),
        Box::new(zero_operand(*span)),
    );
    assign(ctx, holds, rvalue, span);
    switch_on(ctx, Operand::Copy(Place::new(holds)), targets, span);
    Ok(())
}

/// Replace the loop variable by its `successor()`: the new value is held in
/// the variable, and the one it replaces is released.
fn emit_successor_step(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    span: &Span,
) -> Result<(), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let method = (SUCCESSOR_METHOD_NAME, stepped.element.clone());
    let next = call_element_method(ctx, stepped, method, stepped.var, span)?;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Reassign(
            Place::new(stepped.var),
            Rvalue::Use(Operand::Copy(Place::new(next))),
        ),
        span: *span,
    });
    ctx.emit_temp_drop(next, watermark, *span);
    Ok(())
}

/// Call the element type's method `name`, which takes no argument and returns
/// a `result_ty`, on `receiver`, into a new temp, dispatched as a written
/// call on a value of the element type is.
fn call_element_method(
    ctx: &mut LoweringContext,
    stepped: &SteppedLoop,
    (name, result_ty): (&str, Type),
    receiver: Local,
    span: &Span,
) -> Result<Local, LoweringError> {
    let result = ctx.push_temp(result_ty, *span);
    let call = HeldReceiverCall {
        receiver_ty: &stepped.element,
        method_name: name,
        args: vec![Operand::Copy(Place::new(receiver))],
    };
    emit_held_receiver_method_call(ctx, call, &Place::new(result), *span)?;
    Ok(result)
}
