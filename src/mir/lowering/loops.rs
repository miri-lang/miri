// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Loop and control-flow lowering — if, while, for, break, continue.

use std::rc::Rc;

use crate::ast::expression::Expression;
use crate::ast::statement::{IfStatementType, Statement};
use crate::ast::types::{ELEMENT_AT_METHOD_NAME, LENGTH_METHOD_NAME};
use crate::ast::{
    BinaryOp, BuiltinCollectionKind, ExpressionKind, RangeExpressionType, Type, TypeKind,
    VariableDeclaration, WhileStatementType,
};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::Symbol;
use crate::mir::{
    BinOp, Constant, Discriminant, Operand, Place, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};

use super::expression::range_expr::{range_fields, written_inclusivity, written_range_end_test};
use super::stepped_range::{lower_for_over_stepped_range, lower_for_over_stepped_range_value};
use super::{lower_expression, lower_statement, LoweringContext};
use crate::type_checker::{IterationPlan, PositionalAccess, ValueStepping};

pub fn lower_break(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    let (target, scope_depth) = match ctx.loop_stack.last() {
        Some(lc) => (lc.break_target, lc.scope_depth),
        None => return Err(LoweringError::break_outside_loop(*span)),
    };
    // Emit StorageDead for all named locals introduced inside the loop body
    // (scopes opened after enter_loop was called). RC insertion converts these into
    // DecRef operations, ensuring managed locals (e.g. Strings from match branches)
    // are properly released when break exits the scope early.
    ctx.emit_break_cleanup(scope_depth, *span);
    ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, *span));
    Ok(())
}

pub fn lower_continue(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    let (target, scope_depth) = match ctx.loop_stack.last() {
        Some(lc) => (lc.continue_target, lc.scope_depth),
        None => return Err(LoweringError::continue_outside_loop(*span)),
    };
    // Same cleanup as break: emit StorageDead for in-loop-scope locals so that
    // managed values are DecRef'd before the next iteration overwrites them.
    ctx.emit_break_cleanup(scope_depth, *span);
    ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, *span));
    Ok(())
}

pub fn lower_if(
    ctx: &mut LoweringContext,
    span: &Span,
    cond: &Expression,
    then_block: &Statement,
    else_block_opt: &Option<Box<Statement>>,
    if_type: &IfStatementType,
) -> Result<(), LoweringError> {
    let cond_op = lower_expression(ctx, cond, None)?;

    // Create blocks
    let then_bb = ctx.new_basic_block();
    let else_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();

    let (target_val, other_target) = match if_type {
        IfStatementType::If => (1, else_bb),
        IfStatementType::Unless => (0, else_bb),
    };

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Discriminant::from(target_val), then_bb)],
            otherwise: other_target,
        },
        *span,
    ));

    ctx.set_current_block(then_bb);
    lower_branch_into_join(ctx, Some(then_block), join_bb, *span)?;

    ctx.set_current_block(else_bb);
    lower_branch_into_join(ctx, else_block_opt.as_deref(), join_bb, *span)?;

    ctx.set_current_block(join_bb);
    Ok(())
}

/// Lower an optional branch statement and, if it didn't terminate itself,
/// `goto join_bb`.
fn lower_branch_into_join(
    ctx: &mut LoweringContext,
    stmt: Option<&Statement>,
    join_bb: crate::mir::BasicBlock,
    span: Span,
) -> Result<(), LoweringError> {
    if let Some(s) = stmt {
        lower_statement(ctx, s)?;
    }
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: join_bb },
            span,
        ));
    }
    Ok(())
}

/// A block that only jumps back to `header`: the one back-edge of a loop.
///
/// Every `continue` and the fall-through end of the body branch here rather
/// than to the header directly, so the header has a single latch — the shape
/// a structured target (WGSL's `loop { ... continuing { ... } }`) requires.
/// A `for` loop's increment block plays the same role.
pub(super) fn new_latch_block(
    ctx: &mut LoweringContext,
    header: crate::mir::BasicBlock,
    span: Span,
) -> crate::mir::BasicBlock {
    let latch_bb = ctx.new_basic_block();
    ctx.body.basic_blocks[latch_bb.0].terminator = Some(Terminator::new(
        TerminatorKind::Goto { target: header },
        span,
    ));
    latch_bb
}

/// `operand`, a boolean, negated into a temporary.
fn negated(ctx: &mut LoweringContext, operand: Operand, span: Span) -> Operand {
    let temp = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(temp),
            Rvalue::UnaryOp(crate::mir::UnOp::Not, Box::new(operand)),
        ),
        span,
    });
    Operand::Copy(Place::new(temp))
}

/// The boolean constant `value`.
fn bool_constant(value: bool, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Boolean, span),
        literal: crate::ast::literal::Literal::Boolean(value),
    }))
}

pub fn lower_while(
    ctx: &mut LoweringContext,
    span: &Span,
    cond: &Expression,
    body: &Statement,
    while_type: &WhileStatementType,
) -> Result<(), LoweringError> {
    // While/Until: Header (cond) -> Body -> Latch -> Header
    // DoWhile/DoUntil: Header (true) -> Body -> Cond -> Latch -> Header
    // Forever: Header (true) -> Body -> Latch -> Header

    match while_type {
        WhileStatementType::While | WhileStatementType::Until => {
            let header_bb = ctx.new_basic_block();
            let body_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block();

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: header_bb },
                *span,
            ));

            ctx.set_current_block(header_bb);
            let cond_op = lower_expression(ctx, cond, None)?;
            // Every loop header enters its body on `true`: an `until` tests
            // the negated condition, the shape a structured loop (`loop { if
            // (!test) { break; } ... }`) is built from on every backend.
            let test = match while_type {
                WhileStatementType::Until => negated(ctx, cond_op, cond.span),
                _ => cond_op,
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: test,
                    targets: vec![(Discriminant::bool_true(), body_bb)],
                    otherwise: exit_bb,
                },
                *span,
            ));

            let latch_bb = new_latch_block(ctx, header_bb, *span);
            ctx.enter_loop(exit_bb, latch_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body)?;
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: latch_bb },
                    *span,
                ));
            }
            ctx.exit_loop();

            ctx.set_current_block(exit_bb);
        }
        WhileStatementType::DoWhile | WhileStatementType::DoUntil => {
            // The body runs first, under a header testing `true` like a
            // `forever`; the condition at the end either takes the latch back
            // to the header or leaves the loop.
            let header_bb = ctx.new_basic_block();
            let body_bb = ctx.new_basic_block();
            let cond_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block();

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: header_bb },
                *span,
            ));
            ctx.set_current_block(header_bb);
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: bool_constant(true, *span),
                    targets: vec![(Discriminant::bool_true(), body_bb)],
                    otherwise: exit_bb,
                },
                *span,
            ));
            let latch_bb = new_latch_block(ctx, header_bb, *span);

            ctx.enter_loop(exit_bb, cond_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body)?;
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: cond_bb },
                    *span,
                ));
            }
            ctx.exit_loop();

            ctx.set_current_block(cond_bb);
            let cond_op = lower_expression(ctx, cond, None)?;
            let repeat = match while_type {
                WhileStatementType::DoUntil => negated(ctx, cond_op, cond.span),
                _ => cond_op,
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: repeat,
                    targets: vec![(Discriminant::bool_true(), latch_bb)],
                    otherwise: exit_bb,
                },
                *span,
            ));

            ctx.set_current_block(exit_bb);
        }
        WhileStatementType::Forever => {
            // A header testing `true` gives the loop the same shape as a
            // `while`: without it the body's first block is the back-edge
            // target, and a branch at the top of the body (`if k == i: break`)
            // reads as the loop's own exit test.
            let header_bb = ctx.new_basic_block();
            let body_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block(); // Only reachable via break

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: header_bb },
                *span,
            ));
            ctx.set_current_block(header_bb);
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: bool_constant(true, *span),
                    targets: vec![(Discriminant::bool_true(), body_bb)],
                    otherwise: exit_bb,
                },
                *span,
            ));

            let latch_bb = new_latch_block(ctx, header_bb, *span);
            ctx.enter_loop(exit_bb, latch_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body)?;
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: latch_bb },
                    *span,
                ));
            }
            ctx.exit_loop();
            // exit_bb is potentially unreachable unless there's a break,
            // but we set it as current for subsequent statements.
            ctx.set_current_block(exit_bb);
        }
    }
    Ok(())
}

/// The variables a for-loop over an iterable binds.
pub(super) struct LoopBindings {
    /// The element each pass reads.
    pub element: crate::mir::Local,
    /// The index, or the iterator's `Entries` value, when the loop names a
    /// second variable.
    pub secondary: Option<crate::mir::Local>,
    /// Bindings holding a reference-counted value. Each pass reads a fresh
    /// reference into them, so each pass owns and releases them.
    pub per_pass: Vec<crate::mir::Local>,
    /// Names of the `per_pass` bindings, which no scope unbinds on its own.
    pub per_pass_names: Vec<String>,
}

/// Bind the loop's element and, when declared, its second variable of type
/// `secondary_ty`.
///
/// The second variable is bound first so an unmanaged one keeps the local it
/// has always had.
pub(super) fn bind_loop_variables(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable_ty: &Option<Type>,
    secondary_ty: Option<Type>,
) -> LoopBindings {
    let elem_ty = resolve_loop_elem_type(ctx, span, iterable_ty);
    let mut per_pass = Vec::new();
    let mut per_pass_names = Vec::new();
    let mut bind = |ctx: &mut LoweringContext, decl: &VariableDeclaration, ty: Type| {
        let is_managed = ctx.is_counted(&ty.kind);
        let local = setup_loop_variable(ctx, decl, ty, is_managed, span);
        if is_managed {
            per_pass.push(local);
            per_pass_names.push(decl.name.clone());
        }
        local
    };
    let secondary = decls
        .get(1)
        .zip(secondary_ty)
        .map(|(decl, ty)| bind(ctx, decl, ty));
    let element = bind(ctx, &decls[0], elem_ty);
    LoopBindings {
        element,
        secondary,
        per_pass,
        per_pass_names,
    }
}

/// Resolve the element type produced by iterating the loop's iterable.
fn resolve_loop_elem_type(
    ctx: &mut LoweringContext,
    span: &Span,
    iterable_ty: &Option<Type>,
) -> Type {
    let Some(ty) = iterable_ty else {
        return Type::new(TypeKind::Int, *span);
    };
    match &ty.kind {
        TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        TypeKind::Tuple(elem_type_exprs) if !elem_type_exprs.is_empty() => {
            super::resolve_argument_type(ctx.type_checker, &elem_type_exprs[0])
        }
        TypeKind::Custom(name, Some(args))
            if (BuiltinCollectionKind::from_name(name).is_some()
                || name == crate::ast::types::TUPLE_TYPE_NAME)
                && !args.is_empty() =>
        {
            super::resolve_argument_type(ctx.type_checker, &args[0])
        }
        // A class implementing `Iterable<T>` yields `T`. Falling through to the
        // iterable's own type would give the loop variable the class's type, so a
        // managed element would be released through the wrong drop path while its
        // own allocation was never released.
        TypeKind::Custom(name, _)
            if BuiltinCollectionKind::from_name(name).is_none()
                && name != crate::ast::types::TUPLE_TYPE_NAME =>
        {
            ctx.type_checker
                .iterable_element_type(ty)
                .unwrap_or_else(|| ty.clone())
        }
        _ => ty.clone(),
    }
}

/// Register loop variable for managed types, or use push_local for primitives.
fn setup_loop_variable(
    ctx: &mut LoweringContext,
    decl: &VariableDeclaration,
    elem_ty: Type,
    elem_is_managed: bool,
    span: &Span,
) -> crate::mir::Local {
    if elem_is_managed {
        let local = ctx.push_temp(elem_ty, *span);
        if !ctx.is_release {
            ctx.body.local_decls[local.0].name = Some(Rc::from(decl.name.as_str()));
        }
        ctx.body.local_decls[local.0].is_user_variable = true;
        ctx.bind_local_name(decl.name.clone(), local);
        local
    } else {
        ctx.push_local(decl.name.clone(), elem_ty, *span)
    }
}

/// The bodies a `for` loop over a class walked by position calls.
struct IterableMethods {
    length: Symbol,
    element_at: Symbol,
}

/// The bodies a `for` loop walking `access` calls, or `None` when the
/// language indexes the iterable itself.
///
/// Each is the body a written call to the method reaches: for an
/// instantiation of a generic class, the one compiled at its type arguments.
/// The shared body sees its element only as a type parameter, and so hands it
/// over as one value word whatever its real width.
fn resolve_iterable_methods(
    ctx: &mut LoweringContext,
    iterable_id: usize,
    access: &PositionalAccess,
) -> Option<IterableMethods> {
    let class_name = match access {
        PositionalAccess::Methods { class } => class,
        PositionalAccess::Language => return None,
    };
    let receiver_ty = ctx.recorded_type(iterable_id)?;
    Some(IterableMethods {
        length: iterable_method_symbol(ctx, &receiver_ty, class_name, LENGTH_METHOD_NAME),
        element_at: iterable_method_symbol(ctx, &receiver_ty, class_name, ELEMENT_AT_METHOD_NAME),
    })
}

/// The symbol a `for` loop over a `receiver_ty` value of class `class_name`
/// calls for `method_name`.
///
/// A class that inherits the method gets no copy of it: the body belongs to the
/// nearest ancestor that declares it, which [`operator_method_callee`] names at
/// the type arguments the `extends` chain gives it. A class that declares or
/// overrides the method answers for itself, so each method is resolved
/// separately — an override of one does not move another.
pub(super) fn iterable_method_symbol(
    ctx: &mut LoweringContext,
    receiver_ty: &Type,
    class_name: &str,
    method_name: &str,
) -> Symbol {
    let declared = crate::type_checker::context::class_method_declaration(
        class_name,
        method_name,
        ctx.type_checker.type_definitions(),
    )
    .map(|(owner, method)| (owner.to_string(), method.clone()));
    match declared {
        Some((owner, method)) => {
            super::method_dispatch::operator_method_callee(
                ctx,
                receiver_ty,
                &owner,
                method_name,
                &method,
            )
            .0
        }
        None => Symbol::method(class_name, &[], method_name, &[]),
    }
}

/// Emit length check and loop header condition.
/// Compute the iterable's length into `len_temp`: a `{Class}_length` runtime
/// call for collection classes, else a direct `Rvalue::Len`.
fn emit_loop_length(
    ctx: &mut LoweringContext,
    len_temp: crate::mir::Local,
    list_local: crate::mir::Local,
    iterable: Option<&IterableMethods>,
    span: &Span,
) {
    let Some(methods) = iterable else {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(len_temp), Rvalue::Len(Place::new(list_local))),
            span: *span,
        });
        return;
    };
    let func_op = Operand::function(methods.length.clone(), *span);
    // A class method is a body this compilation lowers, never a runtime export,
    // so it always takes the implicit trailing allocator.
    let mut args = vec![Operand::Copy(Place::new(list_local))];
    if let Some(&allocator) = ctx.variable_map.get("allocator") {
        args.push(Operand::Copy(Place::new(allocator)));
    }
    let after_len_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            flags: crate::mir::CallFlags::default(),
            destination: Place::new(len_temp),
            target: Some(after_len_bb),
        },
        *span,
    ));
    ctx.set_current_block(after_len_bb);
}

#[allow(clippy::too_many_arguments)]
fn emit_loop_header(
    ctx: &mut LoweringContext,
    list_local: crate::mir::Local,
    iterable: Option<&IterableMethods>,
    idx_var: crate::mir::Local,
    idx_ty: &Type,
    body_bb: crate::mir::BasicBlock,
    exit_bb: crate::mir::BasicBlock,
    span: &Span,
) -> Result<(), LoweringError> {
    let len_temp = ctx.push_temp(idx_ty.clone(), *span);
    emit_loop_length(ctx, len_temp, list_local, iterable, span);

    let cond_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(cond_temp),
            Rvalue::BinaryOp(
                BinOp::Lt,
                Box::new(Operand::Copy(Place::new(idx_var))),
                Box::new(Operand::Copy(Place::new(len_temp))),
            ),
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

/// Emit element load and secondary variable assignment within loop body.
/// Emit element load from collection within loop body.
fn emit_element_at_call(
    ctx: &mut LoweringContext,
    loop_var: crate::mir::Local,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    element_at_symbol: &Symbol,
    span: &Span,
) {
    let func_op = Operand::function(element_at_symbol.clone(), *span);
    let after_elem_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: {
                let mut args = vec![
                    Operand::Copy(Place::new(list_local)),
                    Operand::Copy(Place::new(idx_var)),
                ];
                // A class method body, which takes the implicit allocator.
                if let Some(&allocator) = ctx.variable_map.get("allocator") {
                    args.push(Operand::Copy(Place::new(allocator)));
                }
                args
            },
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            flags: crate::mir::CallFlags::default(),
            destination: Place::new(loop_var),
            target: Some(after_elem_bb),
        },
        *span,
    ));
    ctx.set_current_block(after_elem_bb);
}

/// Copy the pass's index into the loop's second variable.
fn emit_secondary_loop_var(
    ctx: &mut LoweringContext,
    idx_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    span: &Span,
) {
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(idx_local),
            Rvalue::Use(Operand::Copy(Place::new(idx_var))),
        ),
        span: *span,
    });
}

/// Load the pass's element, and its index when bound.
fn emit_loop_body_element_load(
    ctx: &mut LoweringContext,
    bindings: &LoopBindings,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    iterable: Option<&IterableMethods>,
    span: &Span,
) {
    let loop_var = bindings.element;
    if let Some(methods) = iterable {
        emit_element_at_call(
            ctx,
            loop_var,
            list_local,
            idx_var,
            &methods.element_at,
            span,
        );
    } else {
        let mut indexed_place = Place::new(list_local);
        indexed_place
            .projection
            .push(crate::mir::PlaceElem::index(idx_var));
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(loop_var),
                Rvalue::Use(Operand::Copy(indexed_place)),
            ),
            span: *span,
        });
    }

    if let Some(secondary) = bindings.secondary {
        emit_secondary_loop_var(ctx, secondary, idx_var, span);
    }
}

/// Lower one pass of the loop: load its bindings, then run the body.
///
/// The pass opens a scope of its own, inside the loop, that owns every managed
/// binding. Falling off the end of the body releases them through that scope,
/// and `break`, `continue` and `return` release them through the same scope's
/// early-exit cleanup, so each reference a pass reads is released exactly once
/// whichever way the pass ends.
fn lower_loop_pass(
    ctx: &mut LoweringContext,
    bindings: &LoopBindings,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    iterable: Option<&IterableMethods>,
    body: &Statement,
    span: &Span,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    for &local in &bindings.per_pass {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageLive(Place::new(local)),
            span: *span,
        });
        ctx.register_scope_temp(local);
    }
    emit_loop_body_element_load(ctx, bindings, list_local, idx_var, iterable, span);
    lower_statement(ctx, body)?;
    ctx.pop_scope(*span);
    Ok(())
}

/// Emit the index increment and the jump back to the loop header.
fn emit_loop_increment(
    ctx: &mut LoweringContext,
    idx_var: crate::mir::Local,
    idx_ty: &Type,
    header_bb: crate::mir::BasicBlock,
    span: &Span,
) {
    let one_lit = crate::ast::literal::IntegerLiteral::from_type_kind(&idx_ty.kind, 1)
        .unwrap_or(crate::ast::literal::IntegerLiteral::I64(1));
    let one = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: idx_ty.clone(),
        literal: crate::ast::literal::Literal::Integer(one_lit),
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(idx_var),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(idx_var))),
                Box::new(one),
            ),
        ),
        span: *span,
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));
}

/// Lower `for x in c` over a value the type checker's iteration plan walks
/// by position or through its iterator.
///
/// The plan is the one the checker settled for the `for` statement; the
/// types it names are written in a generic body's own parameters, so they are
/// read at the active instantiation.
fn lower_for_over_iterable(
    ctx: &mut LoweringContext,
    walked: (usize, &Expression),
    decls: &[VariableDeclaration],
    body: &Statement,
    span: &Span,
) -> Result<(), LoweringError> {
    let (statement, iterable) = walked;
    let plan = ctx
        .type_checker
        .recorded_iteration_plan(statement)
        .cloned()
        .ok_or_else(|| {
            LoweringError::internal(
                crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
                "a `for` loop over a collection reached lowering with no iteration plan"
                    .to_string(),
                *span,
            )
        })?;
    match plan {
        IterationPlan::Protocol(walk) => {
            let loop_parts = super::iterator_loop::IteratorLoop {
                decls,
                iterable,
                walk,
                body,
            };
            super::iterator_loop::lower_for_through_iterator(ctx, loop_parts, span)
        }
        IterationPlan::Indexed(access) => {
            lower_for_by_position(ctx, span, decls, iterable, &access, body)
        }
    }
}

/// Lower `for x in c` over a sequence read by position: from index 0 while
/// the index is below its length, reading each element in place or through
/// the class's `element_at`.
fn lower_for_by_position(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    access: &PositionalAccess,
    body: &Statement,
) -> Result<(), LoweringError> {
    ctx.push_scope();

    let iterable_ty = ctx.recorded_type(iterable.id);
    let index_ty = Type::new(TypeKind::Int, *span);
    let bindings = bind_loop_variables(ctx, span, decls, &iterable_ty, Some(index_ty));
    let list_local = lower_loop_iterable(ctx, iterable, iterable_ty, span)?;

    let idx_ty = Type::new(TypeKind::Int, *span);
    let idx_var = start_index(ctx, &idx_ty, span);

    let header_bb = ctx.new_basic_block();
    let body_bb = ctx.new_basic_block();
    let increment_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));

    let iterable_methods = resolve_iterable_methods(ctx, iterable.id, access);

    ctx.set_current_block(header_bb);
    emit_loop_header(
        ctx,
        list_local,
        iterable_methods.as_ref(),
        idx_var,
        &idx_ty,
        body_bb,
        exit_bb,
        span,
    )?;

    ctx.enter_loop(exit_bb, increment_bb);
    ctx.set_current_block(body_bb);
    lower_loop_pass(
        ctx,
        &bindings,
        list_local,
        idx_var,
        iterable_methods.as_ref(),
        body,
        span,
    )?;
    lower_branch_into_join(ctx, None, increment_bb, *span)?;
    ctx.exit_loop();

    ctx.set_current_block(increment_bb);
    emit_loop_increment(ctx, idx_var, &idx_ty, header_bb, span);

    ctx.set_current_block(exit_bb);
    for name in &bindings.per_pass_names {
        ctx.variable_map.remove(name.as_str());
    }
    ctx.pop_scope(*span);
    Ok(())
}

/// A new index temp of type `idx_ty`, set to 0.
fn start_index(ctx: &mut LoweringContext, idx_ty: &Type, span: &Span) -> crate::mir::Local {
    let idx_var = ctx.push_temp(idx_ty.clone(), *span);
    let zero_lit = crate::ast::literal::IntegerLiteral::from_type_kind(&idx_ty.kind, 0)
        .unwrap_or(crate::ast::literal::IntegerLiteral::I64(0));
    let zero = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: idx_ty.clone(),
        literal: crate::ast::literal::Literal::Integer(zero_lit),
    }));
    assign(ctx, idx_var, Rvalue::Use(zero), span);
    idx_var
}

/// Evaluate the loop's iterable into a temp the loop's scope owns, so the
/// reference is released when the loop ends — including by a `return` from
/// inside it.
pub(super) fn lower_loop_iterable(
    ctx: &mut LoweringContext,
    iterable: &Expression,
    iterable_ty: Option<Type>,
    span: &Span,
) -> Result<crate::mir::Local, LoweringError> {
    let list_ty = iterable_ty.unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let list_local = ctx.push_temp(list_ty, *span);
    lower_expression(ctx, iterable, Some(Place::new(list_local)))?;
    if !ctx.borrowed_temps.contains(&list_local) && !ctx.is_owned_by_a_scope(list_local) {
        ctx.register_scope_temp(list_local);
    }
    Ok(list_local)
}

pub fn lower_for(
    ctx: &mut LoweringContext,
    (statement, span): (usize, &Span),
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    // Support for: for i in start..end (range) AND for i in [items] (list)

    // Check for IterableObject (e.g., for i in [1,2,3] parsed as Range with IterableObject type)
    if let ExpressionKind::Range(iterable_expr, _, RangeExpressionType::IterableObject) =
        &iterable.node
    {
        if let Some(element_ty) = range_value_element(ctx, iterable_expr) {
            return lower_for_over_range_value(ctx, span, decls, iterable_expr, element_ty, body);
        }
        // The iterable is in the start position, delegate to list/array handling
        return lower_for_over_iterable(ctx, (statement, iterable_expr), decls, body, span);
    }

    // Also handle direct List expressions
    if let ExpressionKind::List(_) = &iterable.node {
        return lower_for_over_iterable(ctx, (statement, iterable), decls, body, span);
    }

    // Also handle direct Array expressions
    if let ExpressionKind::Array(_, _) = &iterable.node {
        return lower_for_over_iterable(ctx, (statement, iterable), decls, body, span);
    }

    let ExpressionKind::Range(start, end_opt, range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "For loop only supports Range or List iterables".to_string(),
            *span,
        ));
    };
    let Some(end) = end_opt.as_ref() else {
        return Err(LoweringError::unsupported_expression(
            "Range iteration requires an upper bound".to_string(),
            *span,
        ));
    };
    let element_ty = ctx.recorded_type(start.id).ok_or_else(|| {
        LoweringError::internal(
            crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
            "a range in a `for` header reached lowering with no recorded element type".to_string(),
            *span,
        )
    })?;
    let range = WrittenRange {
        start,
        end,
        kind: range_type,
    };
    match ctx.type_checker.range_step(&element_ty) {
        Ok(ValueStepping::Machine) => {
            lower_for_over_written_range(ctx, span, &decls[0], range, element_ty, body)
        }
        Ok(ValueStepping::Method) => {
            lower_for_over_stepped_range(ctx, span, &decls[0], range, element_ty, body)
        }
        Ok(ValueStepping::Deferred) | Err(_) => Err(unsteppable_element(&element_ty, *span)),
    }
}

/// The blocks one pass of a range loop runs between: the body, the step that
/// follows it, where `continue` also goes, and the exit `break` takes.
pub(super) struct PassBlocks {
    pub body: crate::mir::BasicBlock,
    pub step: crate::mir::BasicBlock,
    pub exit: crate::mir::BasicBlock,
}

/// Lower `body` as one pass of a range loop: it starts at `blocks.body`,
/// `break` leaves to `blocks.exit`, and `continue` and the end of the pass go
/// to `blocks.step`.
pub(super) fn lower_range_pass(
    ctx: &mut LoweringContext,
    body: &Statement,
    blocks: &PassBlocks,
    span: &Span,
) -> Result<(), LoweringError> {
    ctx.enter_loop(blocks.exit, blocks.step);
    ctx.set_current_block(blocks.body);
    lower_statement(ctx, body)?;
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        goto(ctx, blocks.step, span);
    }
    ctx.exit_loop();
    Ok(())
}

/// A range written in a `for` loop's header: its two bounds and whether it
/// includes its end.
pub(super) struct WrittenRange<'a> {
    pub start: &'a Expression,
    pub end: &'a Expression,
    pub kind: &'a RangeExpressionType,
}

/// The internal error raised when lowering is asked to step through a range of
/// `element` values, which the type checker refuses to step.
fn unsteppable_element(element: &Type, span: Span) -> LoweringError {
    LoweringError::internal(
        crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
        format!("a range of `{element}` reached lowering to be stepped, but it cannot be stepped"),
        span,
    )
}

/// Lower `for i in start..end` over integers, which the machine steps by
/// adding one.
fn lower_for_over_written_range(
    ctx: &mut LoweringContext,
    span: &Span,
    decl: &VariableDeclaration,
    range: WrittenRange,
    loop_var_ty: Type,
    body: &Statement,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    let loop_var = ctx.push_local(decl.name.clone(), loop_var_ty.clone(), *span);
    let start_op = super::sequencing::lower_operand(ctx, range.start)?;
    assign(ctx, loop_var, Rvalue::Use(start_op), span);
    let end_local = range_end_once(ctx, range.end, span)?;

    let header_bb = ctx.new_basic_block();
    let blocks = PassBlocks {
        body: ctx.new_basic_block(),
        step: ctx.new_basic_block(),
        exit: ctx.new_basic_block(),
    };
    goto(ctx, header_bb, span);

    // The header runs a pass while `i < end`, or `i <= end` when the end is
    // included.
    ctx.set_current_block(header_bb);
    let bin_op = written_range_end_test(range.kind)
        .ok_or_else(|| LoweringError::unsupported_range_type(*span))?;
    let keep_going = compare_locals(ctx, bin_op, loop_var, end_local, span);
    switch_on(
        ctx,
        Operand::Copy(Place::new(keep_going)),
        (blocks.body, blocks.exit),
        span,
    );

    // `continue` goes to the step.
    lower_range_pass(ctx, body, &blocks, span)?;

    ctx.set_current_block(blocks.step);
    let inclusive_end = (*range.kind == RangeExpressionType::Inclusive).then_some(end_local);
    let step = RangeStep {
        loop_var,
        loop_var_ty,
        inclusive_end,
        header_bb,
        exit_bb: blocks.exit,
    };
    emit_range_step(ctx, step, span);
    ctx.set_current_block(blocks.exit);
    ctx.pop_scope(*span);
    Ok(())
}

/// The end of a `for` range, evaluated once before the first iteration into a
/// temporary of its own, which every test of the loop reads.
///
/// The end is a value the range yields once, like any operand, so a call
/// written there runs once and the body writing a binding the end names
/// changes no later test. It is copied even when it is a binding already,
/// since the body may write that binding.
fn range_end_once(
    ctx: &mut LoweringContext,
    end: &Expression,
    span: &Span,
) -> Result<crate::mir::Local, LoweringError> {
    let end_op = super::sequencing::lower_operand(ctx, end)?;
    let end_ty = ctx
        .recorded_type(end.id)
        .unwrap_or_else(|| Type::new(TypeKind::Int, *span));
    let end_local = ctx.push_temp(end_ty, *span);
    assign(ctx, end_local, Rvalue::Use(end_op), span);
    Ok(end_local)
}

/// The element type of `iterable` when it is a range value — a range bound to
/// a name, passed in or returned, rather than written in the loop header.
pub(crate) fn range_value_element(ctx: &LoweringContext, iterable: &Expression) -> Option<Type> {
    let ty = ctx.recorded_type(iterable.id)?;
    let element = crate::ast::types::range_element_expression(&ty.kind)?;
    Some(ctx.resolved_argument_type(element))
}

/// Lower `for x in r` over a range value `r`, stepping as its element type
/// steps: the machine adds one to an integer, and any other `Steppable` type
/// gives its own `successor`.
fn lower_for_over_range_value(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    range_value: &Expression,
    element_ty: Type,
    body: &Statement,
) -> Result<(), LoweringError> {
    match ctx.type_checker.range_step(&element_ty) {
        Ok(ValueStepping::Machine) => {
            lower_for_over_integer_range_value(ctx, span, decls, range_value, element_ty, body)
        }
        Ok(ValueStepping::Method) => {
            let decl = &decls[0];
            lower_for_over_stepped_range_value(ctx, span, decl, range_value, element_ty, body)
        }
        Ok(ValueStepping::Deferred) | Err(_) => Err(unsteppable_element(&element_ty, *span)),
    }
}

/// Lower `for i in r` over a range value `r` of integers, which the machine
/// steps by adding one.
///
/// A range value holds its start, its end and whether the end is included, so
/// the loop reads all three from it once and runs from the start while the
/// variable is below the end, or at it when the end is included. Each pass
/// leaves at the end before stepping, as a range written in the header does.
fn lower_for_over_integer_range_value(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    range_value: &Expression,
    element_ty: Type,
    body: &Statement,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    let bounds = range_value_bounds(ctx, range_value, element_ty.clone(), span)?;
    let loop_var = ctx.push_local(decls[0].name.clone(), element_ty.clone(), *span);
    assign(ctx, loop_var, Rvalue::Use(bounds.start.clone()), span);

    let header_bb = ctx.new_basic_block();
    let blocks = PassBlocks {
        body: ctx.new_basic_block(),
        step: ctx.new_basic_block(),
        exit: ctx.new_basic_block(),
    };
    goto(ctx, header_bb, span);

    ctx.set_current_block(header_bb);
    let keep_going = range_value_condition(ctx, loop_var, &bounds, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(keep_going)),
            targets: vec![(Discriminant::bool_true(), blocks.body)],
            otherwise: blocks.exit,
        },
        *span,
    ));

    lower_range_pass(ctx, body, &blocks, span)?;

    ctx.set_current_block(blocks.step);
    let step = RangeStep {
        loop_var,
        loop_var_ty: element_ty,
        // An exclusive range never reaches its end inside a pass, so leaving
        // at the end is right for both kinds.
        inclusive_end: Some(bounds.end),
        header_bb,
        exit_bb: blocks.exit,
    };
    emit_range_step(ctx, step, span);
    ctx.set_current_block(blocks.exit);
    ctx.pop_scope(*span);
    Ok(())
}

/// The parts of a range value a loop reads once: its start, and the locals
/// holding its end and whether the end is included.
struct RangeValueBounds {
    start: Operand,
    end: crate::mir::Local,
    inclusive: crate::mir::Local,
}

/// Evaluate `range_value` into a temp the loop's scope owns, typed as the
/// `Range<T>` it was recorded at, and read its end and inclusive flag into
/// locals of their own, each from the field the declaration places it in.
fn range_value_bounds(
    ctx: &mut LoweringContext,
    range_value: &Expression,
    element_ty: Type,
    span: &Span,
) -> Result<RangeValueBounds, LoweringError> {
    let parts = range_value_parts(ctx, range_value, span)?;
    let end = ctx.push_temp(element_ty, *span);
    assign(ctx, end, Rvalue::Use(parts.end), span);
    let inclusive = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    assign(ctx, inclusive, Rvalue::Use(parts.inclusive), span);
    Ok(RangeValueBounds {
        start: parts.start,
        end,
        inclusive,
    })
}

/// The three fields of a range value, each read in place from the temp the
/// value was evaluated into.
pub(super) struct RangeValueParts {
    pub start: Operand,
    pub end: Operand,
    pub inclusive: Operand,
}

/// Evaluate `range_value` into a temp the enclosing scope owns, typed as the
/// `Range<T>` it was recorded at, and name its fields in the places the
/// declaration puts them.
pub(super) fn range_value_parts(
    ctx: &mut LoweringContext,
    range_value: &Expression,
    span: &Span,
) -> Result<RangeValueParts, LoweringError> {
    let fields = range_fields(ctx, *span)?;
    let range_ty = ctx.resolved_type(range_value);
    let parts = ctx.push_temp(range_ty, *span);
    lower_expression(ctx, range_value, Some(Place::new(parts)))?;
    if !ctx.borrowed_temps.contains(&parts) && !ctx.is_owned_by_a_scope(parts) {
        ctx.register_scope_temp(parts);
    }
    let part = |index| {
        Operand::Copy(Place {
            local: parts,
            projection: vec![crate::mir::PlaceElem::Field(index)],
        })
    };
    Ok(RangeValueParts {
        start: part(fields.start),
        end: part(fields.end),
        inclusive: part(fields.inclusive),
    })
}

/// The local holding whether `value` has not passed the range's end: it is
/// below the end, or at it and the end is included. A loop runs another pass
/// while this holds.
fn range_value_condition(
    ctx: &mut LoweringContext,
    value: crate::mir::Local,
    bounds: &RangeValueBounds,
    span: &Span,
) -> crate::mir::Local {
    let below = compare_locals(ctx, BinOp::Lt, value, bounds.end, span);
    let at_end = compare_locals(ctx, BinOp::Eq, value, bounds.end, span);
    let at_included_end = compare_locals(ctx, BinOp::BitAnd, at_end, bounds.inclusive, span);
    compare_locals(ctx, BinOp::BitOr, below, at_included_end, span)
}

/// A new boolean local holding `left op right`.
fn compare_locals(
    ctx: &mut LoweringContext,
    op: BinOp,
    left: crate::mir::Local,
    right: crate::mir::Local,
    span: &Span,
) -> crate::mir::Local {
    let result = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    let rvalue = Rvalue::BinaryOp(
        op,
        Box::new(Operand::Copy(Place::new(left))),
        Box::new(Operand::Copy(Place::new(right))),
    );
    assign(ctx, result, rvalue, span);
    result
}

/// The types a range membership test reads: the range's element type, and
/// the type the value and the bounds are compared at, the type holding both
/// that `>=` and `<` compare them at.
pub(crate) struct MembershipTypes {
    pub element: Type,
    pub compared: Type,
}

/// Lower `value in range_value` for a range of numbers into a boolean local.
///
/// A value is in a range when it is at or above the start, and below the end
/// or at an included end: the two comparisons it stands for, made at the type
/// they would be made at. `value` is already lowered, before the range, and
/// already of the compared type. A range written as syntax (`x in a..b`) is
/// never built: its bounds are evaluated in order and compared against
/// directly, the same on every target.
pub(crate) fn lower_range_membership(
    ctx: &mut LoweringContext,
    value: Operand,
    range_value: &Expression,
    types: MembershipTypes,
    span: &Span,
) -> Result<crate::mir::Local, LoweringError> {
    let MembershipTypes { element, compared } = types;
    let value_local = ctx.push_temp(compared.clone(), *span);
    assign(ctx, value_local, Rvalue::Use(value), span);
    let (from_start, before_end) = match written_range_bounds(range_value) {
        Some((start, end, below_end)) => {
            let start = lower_range_bound(ctx, start, &compared)?;
            let end = lower_range_bound(ctx, end, &compared)?;
            let from_start = compare_locals(ctx, BinOp::Ge, value_local, start, span);
            (
                from_start,
                compare_locals(ctx, below_end, value_local, end, span),
            )
        }
        None => {
            let bounds = range_value_bounds(ctx, range_value, element.clone(), span)?;
            let start = converted_local(ctx, bounds.start, (&element, &compared), span);
            let end = Operand::Copy(Place::new(bounds.end));
            let bounds = RangeValueBounds {
                start: Operand::Copy(Place::new(start)),
                end: converted_local(ctx, end, (&element, &compared), span),
                inclusive: bounds.inclusive,
            };
            let from_start = compare_locals(ctx, BinOp::Ge, value_local, start, span);
            (
                from_start,
                range_value_condition(ctx, value_local, &bounds, span),
            )
        }
    };
    Ok(compare_locals(
        ctx,
        BinOp::BitAnd,
        from_start,
        before_end,
        span,
    ))
}

/// `op`, of the type `from`, read into a new local of the type `to`.
fn converted_local(
    ctx: &mut LoweringContext,
    op: Operand,
    (from, to): (&Type, &Type),
    span: &Span,
) -> crate::mir::Local {
    let (op, _) = super::dispatch::conform_operand_to_slot(ctx, op, from.clone(), to, *span);
    let local = ctx.push_temp(to.clone(), *span);
    assign(ctx, local, Rvalue::Use(op), span);
    local
}

/// Lower `value in range_value` over a range of `element`, a type that orders
/// through its `compare`, into a boolean local.
///
/// The test is `start <= value`, asked of the start, then `value < end`, or
/// `value <= end` when the end is included, asked of the value: the two
/// comparisons `<=` and `<` written in the source make. The value and each
/// bound are evaluated once, left to right, and the end is not compared when
/// the start already rules the value out. A bound or a value built for the
/// test is released once both answers are in.
pub(crate) fn lower_compared_range_membership(
    ctx: &mut LoweringContext,
    value: &Expression,
    range_value: &Expression,
    element: Type,
    span: &Span,
) -> Result<crate::mir::Local, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let value_op = super::sequencing::lower_operand(ctx, value)?;
    let bounds = compared_bounds(ctx, range_value, span)?;
    let test = MembershipTest {
        value: (value_op, ctx.resolved_type(value)),
        holds: ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span),
        join: ctx.new_basic_block(),
        // The operands above stay alive across both comparisons, so neither
        // call may release them: each call's watermark sits above them.
        watermark: ctx.body.local_decls.len(),
        span: *span,
    };
    let at_end = ctx.new_basic_block();
    let outside = ctx.new_basic_block();
    let from_start = super::expression::binary_expr::lower_compare_call(
        ctx,
        &BinaryOp::LessThanEqual,
        (bounds.start.clone(), &element),
        test.value.0.clone(),
        (*span, test.watermark),
    )?;
    switch_on(ctx, from_start, (at_end, outside), span);
    ctx.set_current_block(outside);
    hold(ctx, &test, false_operand(*span));
    ctx.set_current_block(at_end);
    lower_end_test(ctx, &test, &bounds)?;
    ctx.set_current_block(test.join);
    for operand in [&test.value.0, &bounds.start, &bounds.end] {
        super::expression::binary_expr::release_membership_operand(ctx, operand, watermark, *span);
    }
    Ok(test.holds)
}

/// The bounds a membership test asks the element type's `compare` about,
/// each evaluated once, and how the test reads the end.
struct ComparedBounds {
    start: Operand,
    end: Operand,
    end_test: EndTest,
}

/// How a membership test compares the value with the end of the range.
pub(super) enum EndTest {
    /// A written range says whether it includes its end.
    Written(bool),
    /// A range value holds whether it does in this boolean local.
    Flag(crate::mir::Local),
}

/// What one range membership test carries from its first comparison to its
/// last: the value and its type, the local holding the answer, the block
/// every path ends in, and the watermark the comparisons release above.
struct MembershipTest {
    value: (Operand, Type),
    holds: crate::mir::Local,
    join: crate::mir::BasicBlock,
    watermark: usize,
    span: Span,
}

/// Evaluate the bounds of `range_value`: the two written expressions in
/// order, or the range value whose fields are read in place.
fn compared_bounds(
    ctx: &mut LoweringContext,
    range_value: &Expression,
    span: &Span,
) -> Result<ComparedBounds, LoweringError> {
    if let Some((start, end, kind)) = written_range_parts(range_value) {
        if let Some(inclusive) = written_inclusivity(kind) {
            let start = super::sequencing::lower_operand(ctx, start)?;
            let end = super::sequencing::lower_operand(ctx, end)?;
            return Ok(ComparedBounds {
                start,
                end,
                end_test: EndTest::Written(inclusive),
            });
        }
    }
    let parts = range_value_parts(ctx, range_value, span)?;
    let inclusive = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    assign(ctx, inclusive, Rvalue::Use(parts.inclusive), span);
    Ok(ComparedBounds {
        start: parts.start,
        end: parts.end,
        end_test: EndTest::Flag(inclusive),
    })
}

/// Compare the value with the end the way `bounds` says, and hold the answer.
fn lower_end_test(
    ctx: &mut LoweringContext,
    test: &MembershipTest,
    bounds: &ComparedBounds,
) -> Result<(), LoweringError> {
    let inclusive = match bounds.end_test {
        EndTest::Written(true) => {
            return hold_ordering(ctx, test, BinaryOp::LessThanEqual, bounds.end.clone())
        }
        EndTest::Written(false) => {
            return hold_ordering(ctx, test, BinaryOp::LessThan, bounds.end.clone())
        }
        EndTest::Flag(inclusive) => inclusive,
    };
    let at_most = ctx.new_basic_block();
    let below = ctx.new_basic_block();
    let flag = Operand::Copy(Place::new(inclusive));
    switch_on(ctx, flag, (at_most, below), &test.span);
    ctx.set_current_block(at_most);
    hold_ordering(ctx, test, BinaryOp::LessThanEqual, bounds.end.clone())?;
    ctx.set_current_block(below);
    hold_ordering(ctx, test, BinaryOp::LessThan, bounds.end.clone())
}

/// Hold `value op end` as the answer of the membership test.
fn hold_ordering(
    ctx: &mut LoweringContext,
    test: &MembershipTest,
    op: BinaryOp,
    end: Operand,
) -> Result<(), LoweringError> {
    let (value, value_ty) = &test.value;
    let answer = super::expression::binary_expr::lower_compare_call(
        ctx,
        &op,
        (value.clone(), value_ty),
        end,
        (test.span, test.watermark),
    )?;
    hold(ctx, test, answer);
    Ok(())
}

/// The boolean `false`, the answer for a value below the start.
fn false_operand(span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Boolean, span),
        literal: crate::ast::literal::Literal::Boolean(false),
    }))
}

/// Write `answer` into the test's answer local and go to its join.
fn hold(ctx: &mut LoweringContext, test: &MembershipTest, answer: Operand) {
    assign(ctx, test.holds, Rvalue::Use(answer), &test.span);
    goto(ctx, test.join, &test.span);
}

/// End the current block by going to `when_true` when `discr` holds and to
/// `otherwise` when it does not.
pub(super) fn switch_on(
    ctx: &mut LoweringContext,
    discr: Operand,
    (when_true, otherwise): (crate::mir::BasicBlock, crate::mir::BasicBlock),
    span: &Span,
) {
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr,
            targets: vec![(Discriminant::bool_true(), when_true)],
            otherwise,
        },
        *span,
    ));
}

/// The start, the end and the comparison against the end of `range_value`
/// when it is range syntax with both bounds written, which membership tests
/// against directly; `None` for a range value.
fn written_range_bounds(range_value: &Expression) -> Option<(&Expression, &Expression, BinOp)> {
    let (start, end, kind) = written_range_parts(range_value)?;
    Some((start, end, written_range_end_test(kind)?))
}

/// The start, the end and the kind of `range_value` when it is range syntax
/// with both bounds written; `None` for a range value.
fn written_range_parts(
    range_value: &Expression,
) -> Option<(&Expression, &Expression, &RangeExpressionType)> {
    let ExpressionKind::Range(start, Some(end), kind) = &range_value.node else {
        return None;
    };
    Some((start, end, kind))
}

/// A bound of range syntax read, the moment it is evaluated, into a local of
/// the type membership compares at. Testing membership in range syntax
/// compares against its bounds directly, so it builds no range value: a
/// kernel has no layout for one.
fn lower_range_bound(
    ctx: &mut LoweringContext,
    bound: &Expression,
    compared: &Type,
) -> Result<crate::mir::Local, LoweringError> {
    let op = super::sequencing::lower_operand(ctx, bound)?;
    let op_ty = ctx.resolved_type(bound);
    Ok(converted_local(ctx, op, (&op_ty, compared), &bound.span))
}

/// Append `local = rvalue` to the current block.
pub(super) fn assign(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    rvalue: Rvalue,
    span: &Span,
) {
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(local), rvalue),
        span: *span,
    });
}

/// End the current block with a jump to `target`.
pub(super) fn goto(ctx: &mut LoweringContext, target: crate::mir::BasicBlock, span: &Span) {
    ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, *span));
}

/// The step at the end of one pass of a range loop.
pub(crate) struct RangeStep {
    pub loop_var: crate::mir::Local,
    pub loop_var_ty: Type,
    /// The local holding the end of an inclusive range; `None` for an
    /// exclusive one.
    pub inclusive_end: Option<crate::mir::Local>,
    pub header_bb: crate::mir::BasicBlock,
    pub exit_bb: crate::mir::BasicBlock,
}

/// Advance a range loop's variable by one and go back to its header.
///
/// An inclusive range leaves once the variable has reached its end, before
/// adding one: the end may be its type's maximum, and one past it does not
/// exist. An exclusive range never steps past an end it is below, so it adds
/// one directly.
pub(crate) fn emit_range_step(ctx: &mut LoweringContext, step: RangeStep, span: &Span) {
    if let Some(end) = step.inclusive_end {
        let at_end = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(at_end),
                Rvalue::BinaryOp(
                    BinOp::Eq,
                    Box::new(Operand::Copy(Place::new(step.loop_var))),
                    Box::new(Operand::Copy(Place::new(end))),
                ),
            ),
            span: *span,
        });
        let advance_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(at_end)),
                targets: vec![(Discriminant::bool_true(), step.exit_bb)],
                otherwise: advance_bb,
            },
            *span,
        ));
        ctx.set_current_block(advance_bb);
    }
    let one_lit = crate::ast::literal::IntegerLiteral::from_type_kind(&step.loop_var_ty.kind, 1)
        .unwrap_or(crate::ast::literal::IntegerLiteral::I64(1));
    let one = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: step.loop_var_ty,
        literal: crate::ast::literal::Literal::Integer(one_lit),
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(step.loop_var),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(step.loop_var))),
                Box::new(one),
            ),
        ),
        span: *span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: step.header_bb,
        },
        *span,
    ));
}
