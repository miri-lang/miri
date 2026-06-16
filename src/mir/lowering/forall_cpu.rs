// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `forall` loops that target CPU (sequential execution).
//!
//! Lowers a CPU-bound `forall` statement to nested sequential `for` loops.
//! Since iterations are independent by definition of `forall`, sequential
//! execution is a correct (if non-parallel) implementation.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::statement::{Statement, VariableDeclaration};
use crate::ast::types::{Type, TypeKind};
use crate::ast::RangeExpressionType;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BinOp, Constant, Discriminant, Local, Operand, Place, Rvalue, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::expression::lower_expression;
use super::statement::lower_statement;

/// Lowers a CPU-bound `forall` loop to nested sequential `for` loops.
///
/// Dispatches on the number of loop variables to the appropriate 1D/2D/3D
/// lowering function. Each builds nested sequential loops with headers,
/// bodies, increments, and exit blocks following the pattern of regular
/// `for` loops in `lower_for`.
pub fn lower_forall_cpu(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    match decls.len() {
        1 => lower_forall_cpu_1d(ctx, span, decls, iterable, body),
        2 => lower_forall_cpu_2d(ctx, span, decls, iterable, body),
        3 => lower_forall_cpu_3d(ctx, span, decls, iterable, body),
        _ => Err(LoweringError::unsupported_expression(
            format!(
                "forall: expected 1, 2, or 3 loop variables, got {}",
                decls.len()
            ),
            *span,
        )),
    }
}

fn lower_forall_cpu_1d(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "forall: iterable must be a bounded numeric range like '0..n'".to_string(),
            *span,
        ));
    };

    ctx.push_scope();

    let loop0 = build_sequential_loop_dimension(ctx, span, &decls[0], start, end, range_type)?;

    ctx.enter_loop(loop0.exit_bb, loop0.increment_bb);
    ctx.set_current_block(loop0.body_bb);
    lower_statement(ctx, body)?;

    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: loop0.increment_bb,
            },
            *span,
        ));
    }
    ctx.exit_loop();

    ctx.set_current_block(loop0.exit_bb);
    ctx.pop_scope(*span);
    Ok(())
}

fn lower_forall_cpu_2d(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let ExpressionKind::Tuple(ranges) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "2D forall requires two comma-separated ranges".to_string(),
            *span,
        ));
    };

    if ranges.len() != 2 {
        return Err(LoweringError::unsupported_expression(
            "2D forall requires exactly two ranges".to_string(),
            *span,
        ));
    }

    let (start_0, end_0, range_type_0) = extract_range(&ranges[0])?;
    let (start_1, end_1, range_type_1) = extract_range(&ranges[1])?;

    ctx.push_scope();

    let loop0 =
        build_sequential_loop_dimension(ctx, span, &decls[0], start_0, end_0, &range_type_0)?;

    ctx.set_current_block(loop0.body_bb);

    ctx.push_scope();
    let loop1 =
        build_sequential_loop_dimension(ctx, span, &decls[1], start_1, end_1, &range_type_1)?;

    ctx.enter_loop(loop1.exit_bb, loop1.increment_bb);
    ctx.set_current_block(loop1.body_bb);
    lower_statement(ctx, body)?;

    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: loop1.increment_bb,
            },
            *span,
        ));
    }
    ctx.exit_loop();

    ctx.set_current_block(loop1.exit_bb);
    ctx.pop_scope(*span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop0.increment_bb,
        },
        *span,
    ));

    ctx.set_current_block(loop0.increment_bb);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop0.header_bb,
        },
        *span,
    ));

    ctx.set_current_block(loop0.exit_bb);
    ctx.pop_scope(*span);
    Ok(())
}

/// Helper to handle the innermost 3D loop body with proper loop context.
fn lower_3d_innermost_body(
    ctx: &mut LoweringContext,
    span: &Span,
    loop2: &LoopBlocks,
    body: &Statement,
) -> Result<(), LoweringError> {
    ctx.enter_loop(loop2.exit_bb, loop2.increment_bb);
    ctx.set_current_block(loop2.body_bb);
    lower_statement(ctx, body)?;

    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: loop2.increment_bb,
            },
            *span,
        ));
    }
    ctx.exit_loop();
    Ok(())
}

fn lower_forall_cpu_3d(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let ExpressionKind::Tuple(ranges) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "3D forall requires three comma-separated ranges".to_string(),
            *span,
        ));
    };

    if ranges.len() != 3 {
        return Err(LoweringError::unsupported_expression(
            "3D forall requires exactly three ranges".to_string(),
            *span,
        ));
    }

    let (start_0, end_0, range_type_0) = extract_range(&ranges[0])?;
    let (start_1, end_1, range_type_1) = extract_range(&ranges[1])?;
    let (start_2, end_2, range_type_2) = extract_range(&ranges[2])?;

    ctx.push_scope();
    let loop0 =
        build_sequential_loop_dimension(ctx, span, &decls[0], start_0, end_0, &range_type_0)?;
    ctx.set_current_block(loop0.body_bb);
    ctx.push_scope();

    let loop1 =
        build_sequential_loop_dimension(ctx, span, &decls[1], start_1, end_1, &range_type_1)?;

    ctx.enter_loop(loop1.exit_bb, loop1.increment_bb);
    ctx.set_current_block(loop1.body_bb);
    ctx.push_scope();

    let loop2 =
        build_sequential_loop_dimension(ctx, span, &decls[2], start_2, end_2, &range_type_2)?;

    lower_3d_innermost_body(ctx, span, &loop2, body)?;
    ctx.set_current_block(loop2.exit_bb);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop1.increment_bb,
        },
        *span,
    ));
    ctx.pop_scope(*span);

    ctx.exit_loop();

    ctx.set_current_block(loop1.exit_bb);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop0.increment_bb,
        },
        *span,
    ));
    ctx.pop_scope(*span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop0.increment_bb,
        },
        *span,
    ));

    ctx.set_current_block(loop0.increment_bb);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop0.header_bb,
        },
        *span,
    ));

    ctx.set_current_block(loop0.exit_bb);
    ctx.pop_scope(*span);
    Ok(())
}

fn extract_range(
    range_expr: &Expression,
) -> Result<(&Expression, &Expression, RangeExpressionType), LoweringError> {
    let ExpressionKind::Range(start, Some(end), range_type) = &range_expr.node else {
        return Err(LoweringError::unsupported_expression(
            "forall range must be a bounded numeric range like 'a..b' or 'a..=b'".to_string(),
            range_expr.span,
        ));
    };
    Ok((start, end, range_type.clone()))
}

/// Represents the basic blocks and loop variable for one sequential loop dimension.
///
/// The loop variable's lifetime is scoped via `push_scope`/`pop_scope` at the caller:
/// - `push_scope()` before calling this function opens the loop's scope.
/// - The loop var is pushed and used through all loop blocks.
/// - `pop_scope()` after `exit_bb` is reached closes the scope and triggers DecRef (Perceus).
///
/// For nested loops (2D/3D), each dimension has its own `push_scope` / `pop_scope` pair,
/// with the innermost dimension's scope destroyed first. This ensures correct Perceus cleanup
/// order even though the loop var itself is immutable.
pub struct LoopBlocks {
    pub header_bb: crate::mir::BasicBlock,
    pub body_bb: crate::mir::BasicBlock,
    pub increment_bb: crate::mir::BasicBlock,
    pub exit_bb: crate::mir::BasicBlock,
    pub loop_var: Local,
}

/// Emits loop initialization: assign start value to loop variable.
fn emit_loop_init(ctx: &mut LoweringContext, loop_var: Local, start_op: Operand, span: &Span) {
    ctx.push_statement(crate::mir::Statement {
        kind: crate::mir::StatementKind::Assign(Place::new(loop_var), Rvalue::Use(start_op)),
        span: *span,
    });
}

/// Emits loop header block: compare current var against end, branch to body or exit.
fn emit_loop_header(
    ctx: &mut LoweringContext,
    span: &Span,
    loop_var: Local,
    end_op: Operand,
    range_type: &RangeExpressionType,
    body_bb: crate::mir::BasicBlock,
    exit_bb: crate::mir::BasicBlock,
) -> Result<(), LoweringError> {
    let current_val = Operand::Copy(Place::new(loop_var));

    let bin_op = match range_type {
        RangeExpressionType::Exclusive => BinOp::Lt,
        RangeExpressionType::Inclusive => BinOp::Le,
        _ => return Err(LoweringError::unsupported_range_type(*span)),
    };

    let bool_ty = Type::new(TypeKind::Boolean, *span);
    let cond_temp = ctx.push_temp(bool_ty, *span);

    ctx.push_statement(crate::mir::Statement {
        kind: crate::mir::StatementKind::Assign(
            Place::new(cond_temp),
            Rvalue::BinaryOp(bin_op, Box::new(current_val), Box::new(end_op)),
        ),
        span: *span,
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(cond_temp)),
            targets: vec![(Discriminant::bool_true(), body_bb)],
            otherwise: exit_bb,
        },
        *span,
    ));
    Ok(())
}

/// Emits loop increment: add 1 to loop variable and jump back to header.
fn emit_loop_increment(
    ctx: &mut LoweringContext,
    span: &Span,
    loop_var: Local,
    loop_var_ty: &Type,
    header_bb: crate::mir::BasicBlock,
) {
    let one = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: loop_var_ty.clone(),
        literal: Literal::Integer(IntegerLiteral::I32(1)),
    }));
    let current_i = Operand::Copy(Place::new(loop_var));

    ctx.push_statement(crate::mir::Statement {
        kind: crate::mir::StatementKind::Assign(
            Place::new(loop_var),
            Rvalue::BinaryOp(BinOp::Add, Box::new(current_i), Box::new(one)),
        ),
        span: *span,
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));
}

/// Connects the exit of an inner loop dimension to the increment of the outer.
/// Builds one sequential loop dimension over a range.
///
/// Creates the necessary basic blocks and emits the loop setup, header condition check,
/// and increment logic. The caller is responsible for lowering the body and managing
/// `enter_loop` / `exit_loop` context.
fn build_sequential_loop_dimension(
    ctx: &mut LoweringContext,
    span: &Span,
    var_decl: &VariableDeclaration,
    start_expr: &Expression,
    end_expr: &Expression,
    range_type: &RangeExpressionType,
) -> Result<LoopBlocks, LoweringError> {
    let loop_var_ty = Type::new(TypeKind::Int, *span);
    let loop_var = ctx.push_local(var_decl.name.clone(), loop_var_ty.clone(), *span);
    let start_op = lower_expression(ctx, start_expr, None)?;

    emit_loop_init(ctx, loop_var, start_op, span);

    let header_bb = ctx.new_basic_block();
    let body_bb = ctx.new_basic_block();
    let increment_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));

    ctx.set_current_block(header_bb);
    let end_op = lower_expression(ctx, end_expr, None)?;
    emit_loop_header(ctx, span, loop_var, end_op, range_type, body_bb, exit_bb)?;

    ctx.set_current_block(increment_bb);
    emit_loop_increment(ctx, span, loop_var, &loop_var_ty, header_bb);

    ctx.set_current_block(exit_bb);

    Ok(LoopBlocks {
        header_bb,
        body_bb,
        increment_bb,
        exit_bb,
        loop_var,
    })
}

/// Checks if a forall body contains any references to GPU-resident variables.
///
/// Returns true if the body captures any identifiers that resolve to
/// gpu-resident locals. Returns false if any capture is unresolved or
/// non-gpu-resident. Used by MIR routing to determine whether a bare
/// `forall` should dispatch to GPU or CPU backend.
pub(crate) fn body_has_gpu_resident_capture(
    ctx: &LoweringContext,
    body: &Statement,
    bound_var_names: &[&str],
) -> bool {
    let mut bound_set = std::collections::HashSet::new();
    for name in bound_var_names {
        bound_set.insert(name.to_string());
    }

    let captures = crate::ast::captures::collect_free_identifiers_excluding(body, &bound_set);

    for name in captures {
        let rc_str = std::rc::Rc::<str>::from(name.as_str());
        if let Some(&local) = ctx.variable_map.get(&rc_str) {
            let residency = ctx.body.local_decls[local.0].residency;
            if matches!(residency, crate::mir::body::BindingResidency::Gpu) {
                return true;
            }
        }
    }

    false
}
