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
        ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, *span));
        Ok(())
    } else {
        Err(LoweringError::break_outside_loop(*span))
    }
}

pub fn lower_continue(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    if let Some(target) = ctx.get_continue_target() {
        ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target }, *span));
        Ok(())
    } else {
        Err(LoweringError::continue_outside_loop(*span))
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
        *span,
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
            *span,
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
            *span,
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
                *span,
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
                *span,
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
                    *span,
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
                *span,
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
                    *span,
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
                *span,
            ));

            ctx.set_current_block(exit_bb);
        }
        WhileStatementType::Forever => {
            let body_bb = ctx.new_basic_block();
            let exit_bb = ctx.new_basic_block(); // Only reachable via break

            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: body_bb },
                *span,
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
        // Extract element type from list/array type parameters
        match &ty.kind {
            TypeKind::List(elem_type_expr) => super::resolve_type(ctx.type_checker, elem_type_expr),
            TypeKind::Array(elem_type_expr, _) => {
                super::resolve_type(ctx.type_checker, elem_type_expr)
            }
            _ => ty.clone(),
        }
    } else {
        Type::new(TypeKind::Int, *span)
    };

    // In lower_for_over_iterable, we want to keep the name if it's user defined.
    // The ctx.push_local logic already handles is_release stripping if we pass the name.
    // However, push_local takes String, and ctx handles the logic.
    let loop_var = ctx.push_local(decl.name.clone(), elem_ty, *span);

    // If there's a second declaration, it's the index variable (for val, idx in ...)
    let idx_loop_var = if decls.len() > 1 {
        let idx_decl = &decls[1];
        let idx_ty = Type::new(TypeKind::Int, *span);
        Some(ctx.push_local(idx_decl.name.clone(), idx_ty, *span))
    } else {
        None
    };

    // Lower the iterable
    let list_op = lower_expression(ctx, iterable, None)?;
    let list_ty = if let Some(ty) = ctx.type_checker.get_type(iterable.id) {
        ty.clone()
    } else {
        Type::new(TypeKind::Void, *span)
    };
    let list_local = ctx.push_temp(list_ty, *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(list_local), Rvalue::Use(list_op)),
        span: *span,
    });

    // Index variable
    let idx_ty = Type::new(TypeKind::Int, *span);
    let idx_var = ctx.push_temp(idx_ty.clone(), *span);

    // Initialize index to 0
    let zero = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: idx_ty.clone(),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(0)),
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(idx_var), Rvalue::Use(zero)),
        span: *span,
    });

    let header_bb = ctx.new_basic_block();
    let body_bb = ctx.new_basic_block();
    let increment_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));

    // Determine if the iterable is a class implementing Iterable trait.
    // If so, we use method calls (ClassName_length, ClassName_element_at)
    // instead of raw Rvalue::Len and PlaceElem::Index.
    let iterable_class: Option<String> = ctx
        .type_checker
        .get_type(iterable.id)
        .and_then(|ty| match &ty.kind {
            TypeKind::String => Some("String".to_string()),
            TypeKind::Custom(name, _) => Some(name.clone()),
            _ => None,
        })
        .filter(|name| {
            matches!(
                ctx.type_checker.global_type_definitions.get(name),
                Some(TypeDefinition::Class(class_def)) if class_def.traits.iter().any(|t| t == "Iterable")
            )
        });

    // Header: Check idx < length
    ctx.set_current_block(header_bb);

    let len_temp = ctx.push_temp(idx_ty.clone(), *span);

    if let Some(ref class_name) = iterable_class {
        // Call ClassName_length(iterable) via MIR terminator
        let length_symbol = format!("{}_length", class_name);
        let func_op = Operand::Constant(Box::new(Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(length_symbol),
        }));
        let after_len_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: {
                    let mut args = vec![Operand::Copy(Place::new(list_local))];
                    if let Some(&allocator) = ctx.variable_map.get("allocator") {
                        args.push(Operand::Copy(Place::new(allocator)));
                    }
                    args
                },
                destination: Place::new(len_temp),
                target: Some(after_len_bb),
            },
            *span,
        ));
        ctx.set_current_block(after_len_bb);
    } else {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(len_temp), Rvalue::Len(Place::new(list_local))),
            span: *span,
        });
    }

    let bool_ty = Type::new(TypeKind::Boolean, *span);
    let cond_temp = ctx.push_temp(bool_ty, *span);
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

    // Body
    ctx.enter_loop(exit_bb, increment_bb);
    ctx.set_current_block(body_bb);

    // Assign loop_var = element_at(idx) or list[idx]
    if let Some(ref class_name) = iterable_class {
        // Call ClassName_element_at(iterable, idx) via MIR terminator
        let element_at_symbol = format!("{}_element_at", class_name);
        let func_op = Operand::Constant(Box::new(Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(element_at_symbol),
        }));
        let after_elem_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: {
                    let mut args = vec![
                        Operand::Copy(Place::new(list_local)),
                        Operand::Copy(Place::new(idx_var)),
                    ];
                    if let Some(&allocator) = ctx.variable_map.get("allocator") {
                        args.push(Operand::Copy(Place::new(allocator)));
                    }
                    args
                },
                destination: Place::new(loop_var),
                target: Some(after_elem_bb),
            },
            *span,
        ));
        ctx.set_current_block(after_elem_bb);
    } else {
        let mut indexed_place = Place::new(list_local);
        indexed_place
            .projection
            .push(crate::mir::PlaceElem::Index(idx_var));
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(loop_var),
                Rvalue::Use(Operand::Copy(indexed_place)),
            ),
            span: *span,
        });
    }

    // If there's an index variable, assign it the current index value
    if let Some(idx_local) = idx_loop_var {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(idx_local),
                Rvalue::Use(Operand::Copy(Place::new(idx_var))),
            ),
            span: *span,
        });
    }

    lower_statement(ctx, body)?;

    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto {
                target: increment_bb,
            },
            *span,
        ));
    }
    ctx.exit_loop();

    // Increment
    ctx.set_current_block(increment_bb);
    let one = Operand::Constant(Box::new(Constant {
        span: *span,
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
        span: *span,
    });

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: header_bb },
        *span,
    ));

    ctx.set_current_block(exit_bb);
    ctx.pop_scope(*span);
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
        let end = match end_opt.as_ref() {
            Some(e) => e,
            None => {
                return Err(LoweringError::unsupported_expression(
                    "Range iteration requires an upper bound".to_string(),
                    *span,
                ));
            }
        };

        ctx.push_scope(); // For the loop variable

        // 1. Initialize loop variable
        // Assumed single declaration for now
        let decl = &decls[0];
        let loop_var_ty = Type::new(TypeKind::Int, *span); // Assuming Int for range
                                                           // Provide the name so push_local can decide to strip it or not based on is_release
        let loop_var = ctx.push_local(decl.name.clone(), loop_var_ty.clone(), *span);
        let start_op = lower_expression(ctx, start, None)?;

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(loop_var), Rvalue::Use(start_op)),
            span: *span,
        });

        let header_bb = ctx.new_basic_block();
        let body_bb = ctx.new_basic_block();
        let increment_bb = ctx.new_basic_block();
        let exit_bb = ctx.new_basic_block();

        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: header_bb },
            *span,
        ));

        // 2. Header: Check condition
        ctx.set_current_block(header_bb);
        let end_op = lower_expression(ctx, end, None)?;
        let current_val = Operand::Copy(Place::new(loop_var));

        // Compare: i < end or i <= end
        let bin_op = match range_type {
            RangeExpressionType::Exclusive => BinOp::Lt,
            RangeExpressionType::Inclusive => BinOp::Le,
            _ => return Err(LoweringError::unsupported_range_type(*span)),
        };

        let bool_ty = Type::new(TypeKind::Boolean, *span);
        let cond_temp = ctx.push_temp(bool_ty, *span);

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
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
                *span,
            ));
        }
        ctx.exit_loop();

        // 4. Increment
        ctx.set_current_block(increment_bb);
        // i = i + 1 - reuse loop_var_ty for the constant
        let one = Operand::Constant(Box::new(Constant {
            span: *span,
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
            span: *span,
        });

        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: header_bb },
            *span,
        ));

        ctx.set_current_block(exit_bb);
        ctx.pop_scope(*span);
    } else {
        return Err(LoweringError::unsupported_expression(
            "For loop only supports Range or List iterables".to_string(),
            *span,
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
                                    *span,
                                ));
                            }

                            let grid_op = lower_expression(ctx, &args[0], None)?;
                            let block_op = lower_expression(ctx, &args[1], None)?;

                            // GPU launch returns void by default.

                            let mut return_ty = Type::new(TypeKind::Void, *span);
                            if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
                                return_ty = ty.clone();
                            }

                            // Use provided dest or create temp
                            let (destination, op) = if let Some(d) = dest {
                                (d.clone(), Operand::Copy(d))
                            } else {
                                let temp = ctx.push_temp(return_ty, *span);
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
                                *span,
                            ));

                            ctx.set_current_block(target_bb);

                            return Ok(op);
                        }
                    }
                }
            }
        }
    }

    // Handle method calls on class types (e.g. s.to_upper(), obj.method(args)).
    // Resolves the class definition from the object's type and emits a call to
    // the mangled function `{ClassName}_{method_name}`.
    if let ExpressionKind::Member(obj, method_expr) = &func.node {
        if let Some(obj_ty) = ctx.type_checker.get_type(obj.id) {
            let class_name = match &obj_ty.kind {
                TypeKind::String => Some("String".to_string()),
                TypeKind::Custom(name, _) => Some(name.clone()),
                _ => None,
            };

            if let Some(class_name) = class_name {
                if let ExpressionKind::Identifier(method_name, _) = &method_expr.node {
                    if let Some(crate::type_checker::context::TypeDefinition::Class(class_def)) =
                        ctx.type_checker.global_type_definitions.get(&class_name)
                    {
                        if let Some(method_info) = class_def.methods.get(method_name.as_str()) {
                            let mangled_name = format!("{}_{}", class_name, method_name);
                            let return_ty = method_info.return_type.clone();

                            let self_op = lower_expression(ctx, obj, None)?;
                            let mut call_args = vec![self_op];
                            for arg in args {
                                call_args.push(lower_expression(ctx, arg, None)?);
                            }

                            // Inject allocator — compiled class methods accept it as their last arg
                            if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
                                call_args.push(Operand::Copy(Place::new(alloc_local)));
                            }

                            let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Identifier, *span),
                                literal: crate::ast::literal::Literal::Identifier(mangled_name),
                            }));

                            let (destination, op) = if let Some(d) = dest {
                                (d.clone(), Operand::Copy(d))
                            } else {
                                let temp = ctx.push_temp(return_ty, *span);
                                let p = Place::new(temp);
                                (p.clone(), Operand::Copy(p))
                            };

                            let target_bb = ctx.new_basic_block();
                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Call {
                                    func: func_op,
                                    args: call_args,
                                    destination,
                                    target: Some(target_bb),
                                },
                                *span,
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

    let func_op = lower_expression(ctx, func, None)?;

    // Try to get function type to check parameters
    let func_ty = ctx.type_checker.get_type(func.id);
    let param_types = if let Some(ty) = func_ty {
        if let TypeKind::Function(func) = &ty.kind {
            Some(func.params.clone())
        } else {
            None
        }
    } else {
        None
    };

    let mut arg_ops = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let op = lower_expression(ctx, arg, None)?;

        let op = if let Some(params) = &param_types {
            if i < params.len() {
                let target_ty = super::resolve_type(ctx.type_checker, &params[i].typ);

                let op_ty = op.ty(&ctx.body);
                if op_ty.kind != target_ty.kind {
                    let temp = ctx.push_temp(target_ty.clone(), arg.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Cast(Box::new(op), target_ty.clone()),
                        ),
                        span: arg.span,
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
    let mut return_ty = Type::new(TypeKind::Void, *span);

    // Attempt to resolve return type from TypeChecker using the Call expression ID
    if let Some(ty) = ctx.type_checker.get_type(call_expr_id) {
        return_ty = ty.clone();
    }

    // Use provided dest or create temp
    let (destination, op) = if let Some(d) = dest {
        // We might want to verify types match, but we trust caller for DPS optimization
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, *span);
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
        *span,
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
    let mut named_args: std::collections::HashMap<&str, Operand> = std::collections::HashMap::new();

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name, op);
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
        } else if let Some(op) = named_args.remove(field_name.as_str()) {
            // Named argument
            op
        } else {
            // Missing field - this should have been caught by type checker
            return Err(LoweringError::missing_struct_field(
                field_name.clone(),
                struct_name.to_string(),
                *span,
            ));
        };

        // Cast if types don't match
        let op_ty = op.ty(&ctx.body);
        let op = if op_ty.kind != field_ty.kind {
            let temp = ctx.push_temp(field_ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Cast(Box::new(op), field_ty.clone()),
                ),
                span: *span,
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    // Create the struct type
    let struct_ty = Type::new(TypeKind::Custom(struct_name.to_string(), None), *span);

    // Assign aggregate to destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(struct_ty.clone(), *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Struct(struct_ty), operands),
        ),
        span: *span,
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
    let mut named_args: std::collections::HashMap<&str, Operand> = std::collections::HashMap::new();

    for arg in args {
        match &arg.node {
            ExpressionKind::NamedArgument(name, value) => {
                let op = lower_expression(ctx, value, None)?;
                named_args.insert(name, op);
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
        } else if let Some(op) = named_args.remove(field_name.as_str()) {
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
            let temp = ctx.push_temp(field_info.ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Cast(Box::new(op), field_info.ty.clone()),
                ),
                span: *span,
            });
            Operand::Copy(Place::new(temp))
        } else {
            op
        };

        operands.push(op);
    }

    // Create the class type
    let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), *span);

    // Assign aggregate to destination
    let (destination, result_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(class_ty.clone(), *span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Class(class_ty), operands),
        ),
        span: *span,
    });

    Ok(result_op)
}

/// Creates a default value operand for a given type.
fn create_default_value(ty: &Type, span: &Span) -> Operand {
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::mir::Constant;

    let literal = match &ty.kind {
        TypeKind::Int | TypeKind::I32 => Literal::Integer(IntegerLiteral::I32(0)),
        TypeKind::I8 => Literal::Integer(IntegerLiteral::I8(0)),
        TypeKind::I16 => Literal::Integer(IntegerLiteral::I16(0)),
        TypeKind::I64 => Literal::Integer(IntegerLiteral::I64(0)),
        TypeKind::I128 => Literal::Integer(IntegerLiteral::I128(0)),
        TypeKind::U8 => Literal::Integer(IntegerLiteral::U8(0)),
        TypeKind::U16 => Literal::Integer(IntegerLiteral::U16(0)),
        TypeKind::U32 => Literal::Integer(IntegerLiteral::U32(0)),
        TypeKind::U64 => Literal::Integer(IntegerLiteral::U64(0)),
        TypeKind::U128 => Literal::Integer(IntegerLiteral::U128(0)),
        TypeKind::Boolean => Literal::Boolean(false),
        TypeKind::String => Literal::String(String::new()),
        _ => Literal::None,
    };

    Operand::Constant(Box::new(Constant {
        span: *span,
        ty: ty.clone(),
        literal,
    }))
}
