// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::Expression;
use crate::ast::statement::{IfStatementType, Statement};
use crate::ast::{
    ExpressionKind, RangeExpressionType, Type, TypeKind, VariableDeclaration, WhileStatementType,
};
use crate::error::syntax::Span;
use crate::mir::{
    AggregateKind, BinOp, Constant, Discriminant, Operand, Place, Rvalue, StatementKind,
    Terminator, TerminatorKind,
};

use super::{lower_expression, lower_statement, LoweringContext};
use crate::error::lowering::LoweringError;
use crate::type_checker::context::{ClassDefinition, StructDefinition, TypeDefinition};

pub fn lower_break(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    if let Some(target) = ctx.get_break_target() {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target },
            span.clone(),
        ));
        Ok(())
    } else {
        Err(LoweringError::break_outside_loop(span.clone()))
    }
}

pub fn lower_continue(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    if let Some(target) = ctx.get_continue_target() {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target },
            span.clone(),
        ));
        Ok(())
    } else {
        Err(LoweringError::continue_outside_loop(span.clone()))
    }
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
        span.clone(),
    ));

    // Lower then block
    ctx.set_current_block(then_bb);
    lower_statement(ctx, then_block)?;
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
        lower_statement(ctx, else_stmt)?;
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
    Ok(())
}

pub fn lower_while(
    ctx: &mut LoweringContext,
    span: &Span,
    cond: &Expression,
    body: &Statement,
    while_type: &WhileStatementType,
) -> Result<(), LoweringError> {
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
            let cond_op = lower_expression(ctx, cond, None)?;
            let (target_val, other_target) = match while_type {
                WhileStatementType::While => (1, exit_bb),
                WhileStatementType::Until => (0, exit_bb),
                _ => unreachable!(),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Discriminant::from(target_val), body_bb)],
                    otherwise: other_target,
                },
                span.clone(),
            ));

            ctx.enter_loop(exit_bb, header_bb);
            ctx.set_current_block(body_bb);
            lower_statement(ctx, body)?;
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
            lower_statement(ctx, body)?;
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
            let cond_op = lower_expression(ctx, cond, None)?;
            let (target_val, other_target) = match while_type {
                WhileStatementType::DoWhile => (1, exit_bb),
                WhileStatementType::DoUntil => (0, exit_bb),
                _ => unreachable!(),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Discriminant::from(target_val), body_bb)],
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
            lower_statement(ctx, body)?;
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
    Ok(())
}

/// Helper to lower for-loops over iterable collections (lists, arrays).
/// Unrolls the iteration by evaluating each element.
fn lower_for_over_iterable(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
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

    // In lower_for_over_iterable, we want to keep the name if it's user defined.
    // The ctx.push_local logic already handles is_release stripping if we pass the name.
    // However, push_local takes String, and ctx handles the logic.
    let loop_var = ctx.push_local(decl.name.clone(), elem_ty, span.clone());

    // Lower the iterable
    let list_op = lower_expression(ctx, iterable, None)?;
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
            targets: vec![(Discriminant::bool_true(), body_bb)],
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

    lower_statement(ctx, body)?;

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

    ctx.set_current_block(exit_bb);
    ctx.pop_scope(span.clone());
    Ok(())
}

pub fn lower_for(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
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
                                                                  // Provide the name so push_local can decide to strip it or not based on is_release
        let loop_var = ctx.push_local(decl.name.clone(), loop_var_ty.clone(), span.clone());
        let start_op = lower_expression(ctx, start, None)?;

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
        let end_op = lower_expression(ctx, end, None)?;
        let current_val = Operand::Copy(Place::new(loop_var));

        // Compare: i < end or i <= end
        let bin_op = match range_type {
            RangeExpressionType::Exclusive => BinOp::Lt,
            RangeExpressionType::Inclusive => BinOp::Le,
            _ => return Err(LoweringError::unsupported_range_type(span.clone())),
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
                targets: vec![(Discriminant::bool_true(), body_bb)],
                otherwise: exit_bb,
            },
            span.clone(),
        ));

        // 3. Body
        ctx.enter_loop(exit_bb, increment_bb); // Continue goes to increment
        ctx.set_current_block(body_bb);
        lower_statement(ctx, body)?;

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
        // i = i + 1 - reuse loop_var_ty for the constant
        let one = Operand::Constant(Box::new(Constant {
            span: span.clone(),
            ty: loop_var_ty,
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

        ctx.set_current_block(exit_bb);
        ctx.pop_scope(span.clone());
    } else {
        return Err(LoweringError::unsupported_expression(
            "For loop only supports Range or List iterables".to_string(),
            span.clone(),
        ));
    }
    Ok(())
}

pub fn lower_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
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
                            let kernel_op = lower_expression(ctx, obj, None)?;

                            if args.len() != 2 {
                                return Err(LoweringError::invalid_gpu_launch_args(
                                    2,
                                    args.len(),
                                    span.clone(),
                                ));
                            }

                            let grid_op = lower_expression(ctx, &args[0], None)?;
                            let block_op = lower_expression(ctx, &args[1], None)?;

                            // GPU launch returns void by default.

                            let mut return_ty = Type::new(TypeKind::Void, span.clone());
                            if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
                                return_ty = ty.clone();
                            }

                            // Use provided dest or create temp
                            let (destination, op) = if let Some(d) = dest {
                                (d.clone(), Operand::Copy(d))
                            } else {
                                let temp = ctx.push_temp(return_ty, span.clone());
                                let p = Place::new(temp);
                                (p.clone(), Operand::Copy(p))
                            };
                            let target_bb = ctx.new_basic_block();

                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::GpuLaunch {
                                    kernel: kernel_op,
                                    grid: grid_op,
                                    block: block_op,
                                    destination,
                                    target: Some(target_bb),
                                },
                                span.clone(),
                            ));

                            ctx.set_current_block(target_bb);

                            return Ok(op);
                        }
                    }
                }
            }
        }
    }

    // Check for struct constructor call
    // The type checker gives struct names the type Meta(Custom(name, ...))
    if let Some(func_ty) = ctx.type_checker.get_type(func.id) {
        if let TypeKind::Meta(inner) = &func_ty.kind {
            if let TypeKind::Custom(type_name, _) = &inner.kind {
                // Look up struct definition
                if let Some(TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    // This is a struct constructor - emit Aggregate instead of Call
                    return lower_struct_constructor(ctx, span, type_name, def, args, dest);
                }
                // Look up class definition
                if let Some(TypeDefinition::Class(def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    // This is a class constructor - emit Aggregate instead of Call
                    return lower_class_constructor(ctx, span, type_name, def, args, dest);
                }
            }
        }
    }

    // Detect print/println with generic (non-string) arguments.
    // The built-in print<T>/println<T> generics accept any type but the
    // runtime expects a *const MiriString.  When called with non-string args
    // we wrap each arg in an f-string conversion so the codegen receives a
    // proper MiriString pointer.
    let is_print_generic = {
        let fname = match &func.node {
            ExpressionKind::Identifier(name, _) => Some(name.as_str()),
            _ => None,
        };
        if let (Some(name), Some(ty)) = (fname, ctx.type_checker.get_type(func.id)) {
            (name == "print" || name == "println")
                && matches!(&ty.kind, TypeKind::Function(Some(gens), _, _) if !gens.is_empty())
        } else {
            false
        }
    };

    let func_op = lower_expression(ctx, func, None)?;

    // Try to get function type to check parameters
    let func_ty = ctx.type_checker.get_type(func.id);
    let param_types = if let Some(ty) = func_ty {
        if let TypeKind::Function(_, params, _) = &ty.kind {
            Some(params.clone())
        } else {
            None
        }
    } else {
        None
    };

    let mut arg_ops = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let op = lower_expression(ctx, arg, None)?;

        // For print<T>/println<T>, wrap non-string args in a FormattedString
        // aggregate so the codegen converts them to MiriString pointers.
        let op = if is_print_generic {
            let op_ty = op.ty(&ctx.body);
            if op_ty.kind != TypeKind::String {
                let str_ty = Type::new(TypeKind::String, arg.span.clone());
                let temp = ctx.push_temp(str_ty, arg.span.clone());
                ctx.push_statement(crate::mir::Statement {
                    kind: StatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::FormattedString, vec![op]),
                    ),
                    span: arg.span.clone(),
                });
                Operand::Copy(Place::new(temp))
            } else {
                op
            }
        } else if let Some(params) = &param_types {
            if i < params.len() {
                let target_ty = super::resolve_type(ctx.type_checker, &params[i].typ);

                let op_ty = op.ty(&ctx.body);
                if op_ty.kind != target_ty.kind {
                    let temp = ctx.push_temp(target_ty.clone(), arg.span.clone());
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Cast(Box::new(op), target_ty.clone()),
                        ),
                        span: arg.span.clone(),
                    });
                    Operand::Copy(Place::new(temp))
                } else {
                    op
                }
            } else {
                op
            }
        } else {
            op
        };
        arg_ops.push(op);
    }

    // Fill in default values for missing arguments
    if let Some(params) = &param_types {
        for param in params.iter().skip(args.len()) {
            if let Some(default_expr) = &param.default_value {
                // Lower the default value expression
                let default_op = lower_expression(ctx, default_expr, None)?;
                arg_ops.push(default_op);
            }
            // If no default and missing, type checker should have caught this error
        }
    }

    // Implicit Allocator Injection at Call Site
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        let already_has_alloc = arg_ops.iter().any(|op| {
            if let Operand::Copy(p) | Operand::Move(p) = op {
                p.local == alloc_local
            } else {
                false
            }
        });

        if !already_has_alloc {
            arg_ops.push(Operand::Copy(Place::new(alloc_local)));
        }
    }

    // Determine return type (void for now, or from type checker)
    let mut return_ty = Type::new(TypeKind::Void, span.clone());

    // Attempt to resolve return type from TypeChecker using the Call expression ID
    if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
        return_ty = ty.clone();
    }

    // Use provided dest or create temp
    let (destination, op) = if let Some(d) = dest {
        // We might want to verify types match, but we trust caller for DPS optimization
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, span.clone());
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let target_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: arg_ops,
            destination,
            target: Some(target_bb),
        },
        span.clone(),
    ));

    ctx.set_current_block(target_bb);

    Ok(op)
}

/// Lowers a struct constructor call to an Aggregate rvalue.
fn lower_struct_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    struct_name: &str,
    def: &StructDefinition,
    args: &[crate::ast::expression::Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Separate positional and named arguments
    let mut positional_args = Vec::new();
    let mut named_args = std::collections::HashMap::new();

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name.clone(), op);
            }
            _ => {
                let op = lower_expression(ctx, arg, None)?;
                positional_args.push(op);
            }
        }
    }

    // Build operands in field declaration order
    let mut operands = Vec::with_capacity(def.fields.len());
    let mut pos_iter = positional_args.into_iter();

    for (field_name, field_ty, _visibility) in &def.fields {
        let op = if let Some(op) = pos_iter.next() {
            // Positional argument
            op
        } else if let Some(op) = named_args.remove(field_name) {
            // Named argument
            op
        } else {
            // Missing field - this should have been caught by type checker
            return Err(LoweringError::missing_struct_field(
                field_name.clone(),
                struct_name.to_string(),
                span.clone(),
            ));
        };

        // Cast if types don't match
        let op_ty = op.ty(&ctx.body);
        let op = if op_ty.kind != field_ty.kind {
            let temp = ctx.push_temp(field_ty.clone(), span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Cast(Box::new(op), field_ty.clone()),
                ),
                span: span.clone(),
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    // Create the struct type
    let struct_ty = Type::new(
        TypeKind::Custom(struct_name.to_string(), None),
        span.clone(),
    );

    // Assign aggregate to destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(struct_ty.clone(), span.clone());
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Struct(struct_ty), operands),
        ),
        span: span.clone(),
    });

    Ok(result_op)
}

/// Lowers a class constructor call to an Aggregate rvalue.
fn lower_class_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    class_name: &str,
    def: &ClassDefinition,
    args: &[crate::ast::expression::Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Separate positional and named arguments
    let mut positional_args = Vec::new();
    let mut named_args = std::collections::HashMap::new();

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name.clone(), op);
            }
            _ => {
                let op = lower_expression(ctx, arg, None)?;
                positional_args.push(op);
            }
        }
    }

    // Build operands in field declaration order (BTreeMap is sorted)
    let mut operands = Vec::with_capacity(def.fields.len());
    let mut pos_iter = positional_args.into_iter();

    for (field_name, field_info) in &def.fields {
        let op = if let Some(op) = pos_iter.next() {
            // Positional argument
            op
        } else if let Some(op) = named_args.remove(field_name) {
            // Named argument
            op
        } else {
            // No argument provided - use default value (zero for now)
            // TODO: Support default field values from class definition
            create_default_value(&field_info.ty, span)
        };

        // Cast if types don't match
        let op_ty = op.ty(&ctx.body);
        let op = if op_ty.kind != field_info.ty.kind {
            let temp = ctx.push_temp(field_info.ty.clone(), span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Cast(Box::new(op), field_info.ty.clone()),
                ),
                span: span.clone(),
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    // Create the class type
    let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), span.clone());

    // Assign aggregate to destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(class_ty.clone(), span.clone());
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Class(class_ty), operands),
        ),
        span: span.clone(),
    });

    Ok(result_op)
}

/// Creates a default value operand for a given type.
fn create_default_value(ty: &Type, span: &Span) -> Operand {
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::mir::Constant;

    let literal = match &ty.kind {
        TypeKind::Int
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I128 => Literal::Integer(IntegerLiteral::I32(0)),
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128 => {
            Literal::Integer(IntegerLiteral::I32(0))
        }
        TypeKind::Boolean => Literal::Boolean(false),
        TypeKind::String => Literal::String(String::new()),
        _ => Literal::None,
    };

    Operand::Constant(Box::new(Constant {
        span: span.clone(),
        ty: ty.clone(),
        literal,
    }))
}
