// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::expression::Expression;
use crate::ast::statement::{IfStatementType, Statement};
use crate::ast::{
    ExpressionKind, RangeExpressionType, Type, TypeKind, VariableDeclaration, WhileStatementType,
};
use crate::error::syntax::Span;
use crate::mir::{
    BinOp, Constant, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind,
};

use super::{lower_expression, lower_statement, LoweringContext};

pub fn lower_break(ctx: &mut LoweringContext, span: &Span) {
    if let Some(target) = ctx.get_break_target() {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target },
            span.clone(),
        ));
    } else {
        panic!("Break outside of loop");
    }
}

pub fn lower_continue(ctx: &mut LoweringContext, span: &Span) {
    if let Some(target) = ctx.get_continue_target() {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target },
            span.clone(),
        ));
    } else {
        panic!("Continue outside of loop");
    }
}

pub fn lower_if(
    ctx: &mut LoweringContext,
    span: &Span,
    cond: &Expression,
    then_block: &Statement,
    else_block_opt: &Option<Box<Statement>>,
    if_type: &IfStatementType,
) {
    let cond_op = lower_expression(ctx, cond);

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
            targets: vec![(target_val, then_bb)],
            otherwise: other_target,
        },
        span.clone(),
    ));

    // Lower then block
    ctx.set_current_block(then_bb);
    lower_statement(ctx, then_block);
    // If the block didn't terminate itself (e.g. return), goto join
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: join_bb },
            span.clone(),
        ));
    }

    // Lower else block
    ctx.set_current_block(else_bb);
    if let Some(else_stmt) = else_block_opt {
        lower_statement(ctx, else_stmt);
    }
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: join_bb },
            span.clone(),
        ));
    }

    ctx.set_current_block(join_bb);
}

pub fn lower_while(
    ctx: &mut LoweringContext,
    span: &Span,
    cond: &Expression,
    body: &Statement,
    while_type: &WhileStatementType,
) {
    // While/Until: Header (cond) -> Body -> Header
    // DoWhile/DoUntil: Body -> Header (cond) -> Body
    // Forever: Body -> Body

    match while_type {
        WhileStatementType::While | WhileStatementType::Until => {
            let header_bb = ctx.new_basic_block();
            let body_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block();

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: header_bb },
                span.clone(),
            ));

            ctx.set_current_block(header_bb);
            let cond_op = lower_expression(ctx, cond);
            let (target_val, other_target) = match while_type {
                WhileStatementType::While => (1, exit_bb),
                WhileStatementType::Until => (0, exit_bb),
                _ => unreachable!(),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(target_val, body_bb)],
                    otherwise: other_target,
                },
                span.clone(),
            ));

            ctx.enter_loop(exit_bb, header_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body);
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: header_bb },
                    span.clone(),
                ));
            }
            ctx.exit_loop();

            ctx.set_current_block(exit_bb);
        }
        WhileStatementType::DoWhile | WhileStatementType::DoUntil => {
            let body_bb = ctx.new_basic_block();
            let cond_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block();

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: body_bb },
                span.clone(),
            ));

            ctx.enter_loop(exit_bb, cond_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body);
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: cond_bb },
                    span.clone(),
                ));
            }
            ctx.exit_loop();

            ctx.set_current_block(cond_bb);
            let cond_op = lower_expression(ctx, cond);
            let (target_val, other_target) = match while_type {
                WhileStatementType::DoWhile => (1, exit_bb),
                WhileStatementType::DoUntil => (0, exit_bb),
                _ => unreachable!(),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(target_val, body_bb)],
                    otherwise: other_target,
                },
                span.clone(),
            ));

            ctx.set_current_block(exit_bb);
        }
        WhileStatementType::Forever => {
            let body_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block(); // Only reachable via break

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: body_bb },
                span.clone(),
            ));

            ctx.enter_loop(exit_bb, body_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body);
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: body_bb },
                    span.clone(),
                ));
            }
            ctx.exit_loop();
            // exit_bb is potentially unreachable unless there's a break,
            // but we set it as current for subsequent statements.
            ctx.set_current_block(exit_bb);
        }
    }
}

/// Helper to lower for-loops over iterable collections (lists, arrays).
/// Unrolls the iteration by evaluating each element.
fn lower_for_over_iterable(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) {
    // For now, use a simple approach: just run the interpreter for this
    // since proper list iteration requires more complex MIR patterns.
    // We'll lower it as: evaluate list, iterate with index.

    ctx.push_scope();

    let decl = &decls[0];
    // Infer element type from type checker or default to Int
    let elem_ty = if let Some(ty) = ctx.type_checker.get_type(iterable.id) {
        // Try to extract element type from list type
        match &ty.kind {
            TypeKind::List(_) | TypeKind::Array(_, _) => {
                // For lists/arrays, element type would need to be extracted from type annotation
                Type::new(TypeKind::Int, span.clone())
            }
            _ => ty.clone(),
        }
    } else {
        Type::new(TypeKind::Int, span.clone())
    };

    let loop_var = ctx.push_local(decl.name.clone(), elem_ty, span.clone());

    // Lower the iterable
    let list_op = lower_expression(ctx, iterable);
    let list_ty = if let Some(ty) = ctx.type_checker.get_type(iterable.id) {
        ty.clone()
    } else {
        Type::new(TypeKind::Void, span.clone())
    };
    let list_local = ctx.push_temp(list_ty, span.clone());
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(list_local), Rvalue::Use(list_op)),
        span: span.clone(),
    });

    // Index variable
    let idx_ty = Type::new(TypeKind::Int, span.clone());
    let idx_var = ctx.push_temp(idx_ty.clone(), span.clone());

    // Initialize index to 0
    let zero = Operand::Constant(Box::new(Constant {
        span: span.clone(),
        ty: idx_ty.clone(),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(0)),
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(idx_var), Rvalue::Use(zero)),
        span: span.clone(),
    });

    let header_bb = ctx.new_basic_block();
    let body_bb = ctx.new_basic_block();
    let increment_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        span.clone(),
    ));

    // Header: Check idx < len(list)
    ctx.set_current_block(header_bb);

    let len_temp = ctx.push_temp(idx_ty.clone(), span.clone());
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(len_temp), Rvalue::Len(Place::new(list_local))),
        span: span.clone(),
    });

    let bool_ty = Type::new(TypeKind::Boolean, span.clone());
    let cond_temp = ctx.push_temp(bool_ty, span.clone());
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(cond_temp),
            Rvalue::BinaryOp(
                BinOp::Lt,
                Box::new(Operand::Copy(Place::new(idx_var))),
                Box::new(Operand::Copy(Place::new(len_temp))),
            ),
        ),
        span: span.clone(),
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(cond_temp)),
            targets: vec![(1, body_bb)],
            otherwise: exit_bb,
        },
        span.clone(),
    ));

    // Body
    ctx.enter_loop(exit_bb, increment_bb);
    ctx.set_current_block(body_bb);

    // Assign loop_var = list[idx]
    let mut indexed_place = Place::new(list_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(idx_var));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(loop_var),
            Rvalue::Use(Operand::Copy(indexed_place)),
        ),
        span: span.clone(),
    });

    lower_statement(ctx, body);

    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: increment_bb,
            },
            span.clone(),
        ));
    }
    ctx.exit_loop();

    // Increment
    ctx.set_current_block(increment_bb);
    let one = Operand::Constant(Box::new(Constant {
        span: span.clone(),
        ty: idx_ty,
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(1)),
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
        span: span.clone(),
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        span.clone(),
    ));

    ctx.pop_scope();
    ctx.set_current_block(exit_bb);
}

pub fn lower_for(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) {
    // Support for: for i in start..end (range) AND for i in [items] (list)

    // Check for IterableObject (e.g., for i in [1,2,3] parsed as Range with IterableObject type)
    if let ExpressionKind::Range(iterable_expr, _, RangeExpressionType::IterableObject) =
        &iterable.node
    {
        // The iterable is in the start position, delegate to list/array handling
        return lower_for_over_iterable(ctx, span, decls, iterable_expr, body);
    }

    // Also handle direct List expressions
    if let ExpressionKind::List(_) = &iterable.node {
        return lower_for_over_iterable(ctx, span, decls, iterable, body);
    }

    // Also handle direct Array expressions
    if let ExpressionKind::Array(_, _) = &iterable.node {
        return lower_for_over_iterable(ctx, span, decls, iterable, body);
    }

    if let ExpressionKind::Range(start, end_opt, range_type) = &iterable.node {
        // Range iteration: for i in start..end
        let end = end_opt.as_ref().expect("Range must have end");

        ctx.push_scope(); // For the loop variable

        // 1. Initialize loop variable
        // Assumed single declaration for now
        let decl = &decls[0];
        let loop_var_ty = Type::new(TypeKind::Int, span.clone()); // Assuming Int for range
        let loop_var = ctx.push_local(decl.name.clone(), loop_var_ty.clone(), span.clone());
        let start_op = lower_expression(ctx, start);

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(loop_var), Rvalue::Use(start_op)),
            span: span.clone(),
        });

        let header_bb = ctx.new_basic_block();
        let body_bb = ctx.new_basic_block();
        let increment_bb = ctx.new_basic_block();
        let exit_bb = ctx.new_basic_block();

        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: header_bb },
            span.clone(),
        ));

        // 2. Header: Check condition
        ctx.set_current_block(header_bb);
        let end_op = lower_expression(ctx, end);
        let current_val = Operand::Copy(Place::new(loop_var));

        // Compare: i < end or i <= end
        let bin_op = match range_type {
            RangeExpressionType::Exclusive => BinOp::Lt,
            RangeExpressionType::Inclusive => BinOp::Le,
            _ => panic!("Unsupported range type for loop"),
        };

        let bool_ty = Type::new(TypeKind::Boolean, span.clone());
        let cond_temp = ctx.push_temp(bool_ty, span.clone());

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(cond_temp),
                Rvalue::BinaryOp(bin_op, Box::new(current_val), Box::new(end_op)),
            ),
            span: span.clone(),
        });

        ctx.set_terminator(Terminator::new(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(cond_temp)),
                targets: vec![(1, body_bb)],
                otherwise: exit_bb,
            },
            span.clone(),
        ));

        // 3. Body
        ctx.enter_loop(exit_bb, increment_bb); // Continue goes to increment
        ctx.set_current_block(body_bb);
        lower_statement(ctx, body);

        if ctx.body.basic_blocks[ctx.current_block.0]
            .terminator
            .is_none()
        {
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto {
                    target: increment_bb,
                },
                span.clone(),
            ));
        }
        ctx.exit_loop();

        // 4. Increment
        ctx.set_current_block(increment_bb);
        // i = i + 1
        let one = Operand::Constant(Box::new(Constant {
            span: span.clone(),
            ty: Type::new(TypeKind::Int, span.clone()),
            literal: crate::ast::literal::Literal::Integer(
                crate::ast::literal::IntegerLiteral::I32(1),
            ),
        }));
        let current_i = Operand::Copy(Place::new(loop_var));

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(loop_var),
                Rvalue::BinaryOp(BinOp::Add, Box::new(current_i), Box::new(one)),
            ),
            span: span.clone(),
        });

        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: header_bb },
            span.clone(),
        ));

        ctx.pop_scope();
        ctx.set_current_block(exit_bb);
    } else {
        panic!("For loop only supports Range or List iterables for now");
    }
}

pub fn lower_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize, // New argument
    func: &Expression,
    args: &[Expression],
) -> Operand {
    // Check for kernel launch: kernel_handle.launch(grid, block)
    if let ExpressionKind::Member(obj, prop) = &func.node {
        if let ExpressionKind::Identifier(name, _) = &prop.node {
            if name == "launch" {
                // Check if the object is of type Kernel
                // We need to resolve the type of 'obj'
                // We can check if TypeChecker says it's Kernel
                // Note: infer_expression puts types in ctx.type_checker.types map by ID.
                if let Some(ty) = ctx.type_checker.get_type(obj.id) {
                    // Check if type name is Kernel
                    if let TypeKind::Custom(type_name, _) = &ty.kind {
                        if type_name == "Kernel" {
                            // This is a GPU kernel launch!
                            let kernel_op = lower_expression(ctx, obj);

                            if args.len() != 2 {
                                panic!("GPU launch expects exactly 2 arguments (grid, block)");
                            }

                            let grid_op = lower_expression(ctx, &args[0]);
                            let block_op = lower_expression(ctx, &args[1]);

                            let mut return_ty = Type::new(TypeKind::Void, span.clone());
                            if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
                                return_ty = ty.clone();
                            }

                            let destination = ctx.push_temp(return_ty, span.clone());
                            let target_bb = ctx.new_basic_block();

                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::GpuLaunch {
                                    kernel: kernel_op,
                                    grid: grid_op,
                                    block: block_op,
                                    destination: Place::new(destination),
                                    target: Some(target_bb),
                                },
                                span.clone(),
                            ));

                            ctx.set_current_block(target_bb);

                            return Operand::Copy(Place::new(destination));
                        }
                    }
                }
            }
        }
    }

    let func_op = lower_expression(ctx, func);
    let arg_ops: Vec<Operand> = args.iter().map(|arg| lower_expression(ctx, arg)).collect();

    // Determine return type (void for now, or from type checker)
    // In a real scenario, we'd lookup the function signature.
    // For now, let's assume Int/Void based on usage or just Void if unknown.
    // Better: check if function is known in TypeChecker
    let mut return_ty = Type::new(TypeKind::Void, span.clone());

    // Attempt to resolve return type from TypeChecker using the Call expression ID
    if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
        return_ty = ty.clone();
    }

    let destination = ctx.push_temp(return_ty, span.clone());
    let target_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: arg_ops,
            destination: Place::new(destination),
            target: Some(target_bb),
        },
        span.clone(),
    ));

    ctx.set_current_block(target_bb);

    Operand::Copy(Place::new(destination))
}
