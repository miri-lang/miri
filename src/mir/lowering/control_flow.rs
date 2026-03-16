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

use super::{helpers::coerce_rvalue, lower_expression, lower_statement, LoweringContext};
use crate::error::lowering::LoweringError;
use crate::type_checker::context::{
    collect_class_fields_all, ClassDefinition, MethodInfo, StructDefinition, TypeDefinition,
};

/// Walk the inheritance chain starting at `class_name` to find the first class
/// that directly declares `method_name`. Returns the defining class name and a
/// clone of its [`MethodInfo`] so the caller can mangle the symbol correctly.
///
/// This is the core of inherited method resolution: if `Dog extends Animal` and
/// only `Animal` defines `speak`, the returned defining class is `"Animal"` and
/// the call is mangled to `Animal_speak`.
fn resolve_inherited_method(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    let mut current = class_name.to_string();
    loop {
        // Resolve the base class name before the borrow of `type_defs` ends.
        let base = match type_defs.get(&current) {
            Some(TypeDefinition::Class(class_def)) => {
                if let Some(method_info) = class_def.methods.get(method_name) {
                    return Some((current, method_info.clone()));
                }
                class_def.base_class.clone()
            }
            _ => return None,
        };
        match base {
            Some(b) => current = b,
            None => return None,
        }
    }
}

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
    let iterable_ty = ctx.type_checker.get_type(iterable.id).cloned();
    let is_map = match iterable_ty.as_ref().map(|t| &t.kind) {
        Some(TypeKind::Map(_, _)) => true,
        Some(TypeKind::Custom(name, _)) if name == "Map" => true,
        _ => false,
    };
    let elem_ty = if let Some(ty) = &iterable_ty {
        // Extract element type from list/array/map/set type parameters
        match &ty.kind {
            TypeKind::List(elem_type_expr) => super::resolve_type(ctx.type_checker, elem_type_expr),
            TypeKind::Array(elem_type_expr, _) => {
                super::resolve_type(ctx.type_checker, elem_type_expr)
            }
            TypeKind::Map(key_type_expr, _) => super::resolve_type(ctx.type_checker, key_type_expr),
            TypeKind::Set(elem_type_expr) => super::resolve_type(ctx.type_checker, elem_type_expr),
            TypeKind::Tuple(elem_type_exprs) if !elem_type_exprs.is_empty() => {
                super::resolve_type(ctx.type_checker, &elem_type_exprs[0])
            }
            TypeKind::Custom(name, Some(args))
                if (name == "Array"
                    || name == "List"
                    || name == "Set"
                    || name == "Map"
                    || name == "Tuple")
                    && !args.is_empty() =>
            {
                super::resolve_type(ctx.type_checker, &args[0])
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

    // If there's a second declaration:
    // - For Map: it's the value variable (for k, v in map)
    // - For others: it's the index variable (for val, idx in list)
    let idx_loop_var = if decls.len() > 1 {
        let idx_decl = &decls[1];
        let var_ty = if is_map {
            match iterable_ty.as_ref().map(|t| &t.kind) {
                Some(TypeKind::Map(_, val_type_expr)) => {
                    super::resolve_type(ctx.type_checker, val_type_expr)
                }
                Some(TypeKind::Custom(name, Some(args))) if name == "Map" && args.len() == 2 => {
                    super::resolve_type(ctx.type_checker, &args[1])
                }
                _ => Type::new(TypeKind::Int, *span),
            }
        } else {
            Type::new(TypeKind::Int, *span)
        };
        Some(ctx.push_local(idx_decl.name.clone(), var_ty, *span))
    } else {
        None
    };

    // Lower the iterable using DPS to avoid an extra Copy (and spurious IncRef).
    // The single reference owned by list_local is released via StorageDead after the loop.
    let list_ty = if let Some(ty) = ctx.type_checker.get_type(iterable.id) {
        ty.clone()
    } else {
        Type::new(TypeKind::Void, *span)
    };
    let list_local = ctx.push_temp(list_ty, *span);
    lower_expression(ctx, iterable, Some(Place::new(list_local)))?;

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
            TypeKind::Map(_, _) => Some("Map".to_string()),
            TypeKind::Set(_) => Some("Set".to_string()),
            TypeKind::Custom(name, _) if name != "Array" && name != "List" && name != "Tuple" => Some(name.clone()),
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
            literal: crate::ast::literal::Literal::Identifier(length_symbol.clone()),
        }));
        let after_len_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: {
                    let mut args = vec![Operand::Copy(Place::new(list_local))];
                    if !length_symbol.starts_with("miri_") {
                        if let Some(&allocator) = ctx.variable_map.get("allocator") {
                            args.push(Operand::Copy(Place::new(allocator)));
                        }
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
            literal: crate::ast::literal::Literal::Identifier(element_at_symbol.clone()),
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
                    if !element_at_symbol.starts_with("miri_") {
                        if let Some(&allocator) = ctx.variable_map.get("allocator") {
                            args.push(Operand::Copy(Place::new(allocator)));
                        }
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

    // If there's a second loop variable, assign it
    if let Some(idx_local) = idx_loop_var {
        if is_map {
            // For Map: call Map_value_at(map, idx) to get the value
            let value_at_symbol = "Map_value_at".to_string();
            let func_op = Operand::Constant(Box::new(Constant {
                span: *span,
                ty: Type::new(TypeKind::Identifier, *span),
                literal: crate::ast::literal::Literal::Identifier(value_at_symbol),
            }));
            let after_val_bb = ctx.new_basic_block();
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
                    destination: Place::new(idx_local),
                    target: Some(after_val_bb),
                },
                *span,
            ));
            ctx.set_current_block(after_val_bb);
        } else {
            // For List/Array/String: assign the loop counter as the index
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(
                    Place::new(idx_local),
                    Rvalue::Use(Operand::Copy(Place::new(idx_var))),
                ),
                span: *span,
            });
        }
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
    // Release the iterable local now that the loop is done.
    ctx.emit_temp_drop(list_local, 0, *span);
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
            // Intercept built-in .length() calls on List/Array/String types.
            // These are handled natively by the codegen via Rvalue::Len which reads
            // the LEN field from the RC+LEN+DATA memory layout.
            if let ExpressionKind::Identifier(method_name, _) = &method_expr.node {
                if method_name == "length"
                    && (matches!(
                        &obj_ty.kind,
                        TypeKind::Tuple(_)
                            | TypeKind::List(_)
                            | TypeKind::Array(_, _)
                            | TypeKind::Map(_, _)
                            | TypeKind::Set(_)
                            | TypeKind::String
                    ) || matches!(
                        &obj_ty.kind,
                        TypeKind::Custom(name, _) if name == "Array" || name == "List" || name == "Map" || name == "Set" || name == "Tuple"
                    ))
                {
                    let obj_watermark = ctx.body.local_decls.len();
                    let obj_op = lower_expression(ctx, obj, None)?;
                    // Only drop obj_local if obj_op is a plain Copy (no field projections).
                    // Perceus inserts IncRef only for projection-free Copy operands; field
                    // projections (e.g. `b.values`) are not IncRef'd by Perceus, so emitting
                    // StorageDead (and thus a balancing DecRef) would prematurely free the value.
                    // For Move operands Perceus does not IncRef, so no drop is needed either.
                    let obj_op_is_copy =
                        matches!(&obj_op, Operand::Copy(p) if p.projection.is_empty());
                    // Create a temp local to hold the object, so we can form Place for Rvalue::Len
                    let obj_local = ctx.push_temp(obj_ty.clone(), *span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
                        span: *span,
                    });

                    let len_ty = Type::new(TypeKind::Int, *span);
                    let (destination, op) = if let Some(d) = dest {
                        (d.clone(), Operand::Copy(d))
                    } else {
                        let temp = ctx.push_temp(len_ty, *span);
                        let p = Place::new(temp);
                        (p.clone(), Operand::Copy(p))
                    };

                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            destination,
                            Rvalue::Len(Place::new(obj_local)),
                        ),
                        span: *span,
                    });

                    if obj_op_is_copy {
                        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
                    }
                    return Ok(op);
                }

                if (method_name == "element_at" || method_name == "get")
                    && (matches!(
                        &obj_ty.kind,
                        TypeKind::Tuple(_) | TypeKind::List(_) | TypeKind::Array(_, _)
                    ) || matches!(
                        &obj_ty.kind,
                        TypeKind::Custom(name, _) if name == "Array" || name == "List" || name == "Tuple"
                    ))
                    && args.len() == 1
                {
                    let obj_watermark = ctx.body.local_decls.len();
                    let obj_op = lower_expression(ctx, obj, None)?;
                    // See comment in the `length` branch: Perceus only IncRefs projection-free
                    // Copy operands, so field projections must not trigger emit_temp_drop.
                    let obj_op_is_copy =
                        matches!(&obj_op, Operand::Copy(p) if p.projection.is_empty());
                    let index_op = lower_expression(ctx, &args[0], None)?;

                    // Create a temp local to hold the object
                    let obj_local = ctx.push_temp(obj_ty.clone(), *span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
                        span: *span,
                    });

                    // Ensure index is in a local for Projection
                    let index_local = match index_op {
                        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
                        _ => {
                            let temp =
                                ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
                            ctx.push_statement(crate::mir::Statement {
                                kind: StatementKind::Assign(
                                    Place::new(temp),
                                    Rvalue::Use(index_op),
                                ),
                                span: args[0].span,
                            });
                            temp
                        }
                    };

                    let mut indexed_place = Place::new(obj_local);
                    indexed_place
                        .projection
                        .push(crate::mir::PlaceElem::Index(index_local));

                    let elem_ty = if let Some(t) = ctx.type_checker.get_type(call_expr_id) {
                        t.clone()
                    } else {
                        Type::new(TypeKind::Int, *span)
                    };

                    let (destination, op) = if let Some(d) = dest {
                        (d.clone(), Operand::Copy(d))
                    } else {
                        let temp = ctx.push_temp(elem_ty, *span);
                        let p = Place::new(temp);
                        (p.clone(), Operand::Copy(p))
                    };

                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            destination,
                            Rvalue::Use(Operand::Copy(indexed_place)),
                        ),
                        span: *span,
                    });

                    // Only drop obj_local when a Copy was used (Perceus will have IncRef'd).
                    if obj_op_is_copy {
                        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
                    }
                    return Ok(op);
                }

                if method_name == "push"
                    && (matches!(&obj_ty.kind, TypeKind::List(_))
                        || matches!(&obj_ty.kind, TypeKind::Custom(name, _) if name == "List"))
                    && args.len() == 1
                {
                    let obj_op = lower_expression(ctx, obj, None)?;
                    let item_op = lower_expression(ctx, &args[0], None)?;

                    // Create a temp local for the item so we can pass its address
                    let item_ty = item_op.ty(&ctx.body);
                    let item_local = ctx.push_temp(item_ty.clone(), args[0].span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_op)),
                        span: args[0].span,
                    });

                    let func_op = Operand::Constant(Box::new(Constant {
                        span: *span,
                        ty: Type::new(TypeKind::Identifier, *span),
                        literal: crate::ast::literal::Literal::Identifier(
                            "miri_rt_list_push".to_string(),
                        ),
                    }));

                    let target_bb = ctx.new_basic_block();
                    // push returns void, but Call requires a destination. Use a dummy.
                    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);

                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::Call {
                            func: func_op,
                            args: vec![obj_op, Operand::Copy(Place::new(item_local))],
                            destination: Place::new(dummy_dest),
                            target: Some(target_bb),
                        },
                        *span,
                    ));

                    ctx.set_current_block(target_bb);
                    return Ok(Operand::Copy(Place::new(dummy_dest)));
                }

                if method_name == "set"
                    && (matches!(&obj_ty.kind, TypeKind::List(_) | TypeKind::Array(_, _))
                        || matches!(&obj_ty.kind, TypeKind::Custom(name, _) if name == "Array" || name == "List"))
                    && args.len() == 2
                {
                    let obj_op = lower_expression(ctx, obj, None)?;
                    let index_op = lower_expression(ctx, &args[0], None)?;
                    let item_op = lower_expression(ctx, &args[1], None)?;

                    // For 'set', we can just use MIR assignment to an indexed place!
                    // obj[index] = item

                    // Create a temp local to hold the object
                    let obj_local = ctx.push_temp(obj_ty.clone(), *span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
                        span: *span,
                    });

                    // Ensure index is in a local for Projection
                    let index_local = match index_op {
                        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
                        _ => {
                            let temp =
                                ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
                            ctx.push_statement(crate::mir::Statement {
                                kind: StatementKind::Assign(
                                    Place::new(temp),
                                    Rvalue::Use(index_op),
                                ),
                                span: args[0].span,
                            });
                            temp
                        }
                    };

                    let mut indexed_place = Place::new(obj_local);
                    indexed_place
                        .projection
                        .push(crate::mir::PlaceElem::Index(index_local));

                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(indexed_place, Rvalue::Use(item_op)),
                        span: *span,
                    });

                    return Ok(Operand::Constant(Box::new(Constant {
                        span: *span,
                        ty: Type::new(TypeKind::Void, *span),
                        literal: crate::ast::literal::Literal::None,
                    })));
                }

                if method_name == "insert"
                    && (matches!(&obj_ty.kind, TypeKind::List(_))
                        || matches!(&obj_ty.kind, TypeKind::Custom(name, _) if name == "List"))
                    && args.len() == 2
                {
                    let obj_op = lower_expression(ctx, obj, None)?;
                    let index_op = lower_expression(ctx, &args[0], None)?;
                    let item_op = lower_expression(ctx, &args[1], None)?;

                    // Create a temp local for the item so we can pass its address
                    let item_ty = item_op.ty(&ctx.body);
                    let item_local = ctx.push_temp(item_ty.clone(), args[1].span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_op)),
                        span: args[1].span,
                    });

                    let func_op = Operand::Constant(Box::new(Constant {
                        span: *span,
                        ty: Type::new(TypeKind::Identifier, *span),
                        literal: crate::ast::literal::Literal::Identifier(
                            "miri_rt_list_insert".to_string(),
                        ),
                    }));

                    let target_bb = ctx.new_basic_block();
                    let result_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);

                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::Call {
                            func: func_op,
                            args: vec![obj_op, index_op, Operand::Copy(Place::new(item_local))],
                            destination: Place::new(result_temp),
                            target: Some(target_bb),
                        },
                        *span,
                    ));

                    ctx.set_current_block(target_bb);
                    return Ok(Operand::Copy(Place::new(result_temp)));
                }
            }

            let class_name = match &obj_ty.kind {
                TypeKind::String => Some("String".to_string()),
                TypeKind::List(_) => Some("List".to_string()),
                TypeKind::Array(_, _) => Some("Array".to_string()),
                TypeKind::Map(_, _) => Some("Map".to_string()),
                TypeKind::Set(_) => Some("Set".to_string()),
                TypeKind::Tuple(_) => Some("Tuple".to_string()),
                TypeKind::Custom(name, _) => Some(name.clone()),
                _ => None,
            };

            if let Some(class_name) = class_name {
                if let ExpressionKind::Identifier(method_name, _) = &method_expr.node {
                    if let Some((defining_class, method_info)) = resolve_inherited_method(
                        &ctx.type_checker.global_type_definitions,
                        &class_name,
                        method_name,
                    ) {
                        let mangled_name = format!("{}_{}", defining_class, method_name);
                        let return_ty = method_info.return_type.clone();

                        // For `super.method()`, the receiver must be `self` (the current
                        // instance), not the super constant (which would lower to a null
                        // pointer via Literal::Identifier). The type checker already resolved
                        // obj_ty to the parent class type so `resolve_inherited_method` above
                        // correctly starts its search from the parent — we only need to
                        // substitute the actual self operand here.
                        let self_op = if matches!(&obj.node, ExpressionKind::Super) {
                            if let Some(&self_local) = ctx.variable_map.get("self") {
                                Operand::Copy(Place::new(self_local))
                            } else {
                                lower_expression(ctx, obj, None)?
                            }
                        } else {
                            lower_expression(ctx, obj, None)?
                        };
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
                    if type_name == "List" {
                        let list_ty = if let Some(call_ty) = ctx.type_checker.get_type(call_expr_id)
                        {
                            call_ty.clone()
                        } else {
                            Type::new(TypeKind::Int, *span)
                        };

                        let (destination, result_op) = if let Some(d) = dest {
                            (d.clone(), Operand::Copy(d))
                        } else {
                            let temp = ctx.push_temp(list_ty.clone(), *span);
                            let p = Place::new(temp);
                            (p.clone(), Operand::Copy(p))
                        };

                        let target_bb = ctx.new_basic_block();

                        if args.len() == 1 {
                            let array_op = lower_expression(ctx, &args[0], None)?;

                            // Track the temp array local so we can emit StorageDead after the call
                            let temp_array_local = match &array_op {
                                Operand::Copy(p) | Operand::Move(p) => Some(p.clone()),
                                _ => None,
                            };

                            // Determine array length, element size, and whether
                            // elements are RC-managed (Option, List, Array, etc.)
                            let mut len_val = 0;
                            let mut elem_size = 8;
                            let mut elems_are_managed = false;
                            if let ExpressionKind::Array(elements, _) = &args[0].node {
                                len_val = elements.len() as i64;
                                if !elements.is_empty() {
                                    if let Some(ty) = ctx.type_checker.get_type(elements[0].id) {
                                        elem_size = compute_elem_size_from_type(&ty.kind);
                                        elems_are_managed = ctx.is_perceus_managed(&ty.kind);
                                    }
                                }
                            }

                            let len_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(len_val),
                                ),
                            }));

                            let size_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(elem_size),
                                ),
                            }));

                            // Use the managed-array variant when elements are
                            // heap-allocated so the list IncRefs them before the
                            // source array's element-drop loop releases its refs.
                            let rt_fn_name = if elems_are_managed {
                                "miri_rt_list_new_from_managed_array"
                            } else {
                                "miri_rt_list_new_from_raw"
                            };
                            let func_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Identifier, *span),
                                literal: crate::ast::literal::Literal::Identifier(
                                    rt_fn_name.to_string(),
                                ),
                            }));

                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Call {
                                    func: func_op,
                                    args: vec![array_op, len_op, size_op],
                                    destination: destination.clone(),
                                    target: Some(target_bb),
                                },
                                *span,
                            ));

                            // The temp array was consumed by miri_rt_list_new_from_raw
                            // (data copied). Emit StorageDead so Perceus inserts DecRef.
                            ctx.set_current_block(target_bb);
                            if let Some(arr_place) = temp_array_local {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: StatementKind::StorageDead(arr_place),
                                    span: *span,
                                });
                            }

                            // Need a new target block since we added statements to the original
                            let final_bb = ctx.new_basic_block();
                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Goto { target: final_bb },
                                *span,
                            ));
                            ctx.set_current_block(final_bb);
                            return Ok(result_op);
                        } else {
                            // Assuming element size is 8 for simplicity, or 0 if it doesn't matter yet
                            let size_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Int, *span),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I64(8),
                                ),
                            }));
                            let func_op = Operand::Constant(Box::new(Constant {
                                span: *span,
                                ty: Type::new(TypeKind::Identifier, *span),
                                literal: crate::ast::literal::Literal::Identifier(
                                    "miri_rt_list_new".to_string(),
                                ),
                            }));
                            ctx.set_terminator(Terminator::new(
                                TerminatorKind::Call {
                                    func: func_op,
                                    args: vec![size_op],
                                    destination: destination.clone(),
                                    target: Some(target_bb),
                                },
                                *span,
                            ));
                        }

                        ctx.set_current_block(target_bb);
                        return Ok(result_op);
                    }

                    if type_name == "Map" || type_name == "Set" {
                        let return_ty =
                            if let Some(call_ty) = ctx.type_checker.get_type(call_expr_id) {
                                call_ty.clone()
                            } else if type_name == "Map" {
                                crate::ast::factory::type_map(
                                    crate::ast::factory::type_void(),
                                    crate::ast::factory::type_void(),
                                )
                            } else {
                                crate::ast::factory::type_set(crate::ast::factory::type_void())
                            };

                        let (destination, result_op) = if let Some(d) = dest {
                            (d.clone(), Operand::Copy(d))
                        } else {
                            let temp = ctx.push_temp(return_ty, *span);
                            let p = Place::new(temp);
                            (p.clone(), Operand::Copy(p))
                        };

                        let aggregate_kind = if type_name == "Map" {
                            AggregateKind::Map
                        } else {
                            AggregateKind::Set
                        };

                        ctx.push_statement(crate::mir::Statement {
                            kind: StatementKind::Assign(
                                destination,
                                Rvalue::Aggregate(aggregate_kind, vec![]),
                            ),
                            span: *span,
                        });

                        return Ok(result_op);
                    }

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

    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        let op = lower_expression(ctx, arg, None)?;

        let op = if let Some(params) = &param_types {
            if i < params.len() {
                let target_ty = super::resolve_type(ctx.type_checker, &params[i].typ);

                let op_ty = op.ty(&ctx.body).clone();
                if op_ty.kind != target_ty.kind {
                    let temp = ctx.push_temp(target_ty.clone(), arg.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            Place::new(temp),
                            coerce_rvalue(op, &op_ty, &target_ty),
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

        // Ensure managed arguments are passed as Copy so that Perceus inserts
        // an IncRef at the call site. The callee owns the reference and releases
        // it via StorageDead on the parameter in finalize_body. Without this, a
        // Move argument is not IncRef'd, the callee's DecRef brings RC to 0, and
        // the caller's reference becomes dangling.
        let op = match op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
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
    let is_runtime_fn = if let ExpressionKind::Identifier(name, _) = &func.node {
        name.starts_with("miri_")
    } else {
        false
    };

    if !is_runtime_fn {
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
            args: arg_ops.clone(),
            destination: destination.clone(),
            target: Some(target_bb),
        },
        *span,
    ));

    ctx.set_current_block(target_bb);

    // Release managed temporaries created while lowering the call arguments.
    let dest_local = destination.local;
    for arg_op in &arg_ops {
        if let Operand::Copy(place) | Operand::Move(place) = arg_op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

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
    let arg_watermark = ctx.body.local_decls.len();
    let mut positional_args = Vec::with_capacity(args.len());
    let mut named_args: std::collections::HashMap<&str, Operand> =
        std::collections::HashMap::with_capacity(args.len());

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
        let op_ty = op.ty(&ctx.body).clone();
        let op = if op_ty.kind != field_ty.kind {
            let temp = ctx.push_temp(field_ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), coerce_rvalue(op, &op_ty, field_ty)),
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

    let dest_local = destination.local;
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            destination,
            Rvalue::Aggregate(AggregateKind::Struct(struct_ty), operands.clone()),
        ),
        span: *span,
    });

    // Release managed temporaries created while lowering the constructor arguments.
    // After the Aggregate assignment, Perceus has IncRef'd them (the struct now owns
    // the references). The caller's temporary locals are no longer needed.
    for op in &operands {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, *span);
            }
        }
    }

    Ok(result_op)
}

/// Lowers a class constructor call to an Aggregate rvalue,
/// then calls the `init` method if one exists.
fn lower_class_constructor(
    ctx: &mut LoweringContext,
    span: &Span,
    class_name: &str,
    def: &ClassDefinition,
    args: &[crate::ast::expression::Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // Resolve init method: own class first, then walk the inheritance chain.
    let init_class_name: Option<String> = {
        if def.methods.get("init").is_some_and(|m| !m.is_abstract) {
            Some(class_name.to_string())
        } else if let Some(base) = &def.base_class {
            resolve_inherited_method(
                &ctx.type_checker.global_type_definitions,
                base,
                "init",
            )
            .filter(|(_, m)| !m.is_abstract)
            .map(|(c, _)| c)
        } else {
            None
        }
    };

    // Collect ALL fields in inheritance order (base class fields first).
    // This defines the canonical memory layout for the class instance.
    let all_fields: Vec<(String, crate::type_checker::context::FieldInfo)> = {
        collect_class_fields_all(def, &ctx.type_checker.global_type_definitions)
            .into_iter()
            .map(|(n, f)| (n.to_string(), f.clone()))
            .collect()
    };

    if let Some(init_class) = init_class_name {
        // When init exists (own or inherited), constructor args are init params.
        // Allocate the object with default field values for ALL fields, then call init.
        let field_defaults: Vec<Operand> = all_fields
            .iter()
            .map(|(_, fi)| create_default_value(&fi.ty, span))
            .collect();

        let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), *span);

        let (destination, result_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(class_ty.clone(), *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                destination.clone(),
                Rvalue::Aggregate(AggregateKind::Class(class_ty), field_defaults),
            ),
            span: *span,
        });

        // Build init call args: self + constructor args + allocator
        let mut call_args = vec![Operand::Copy(destination)];
        let init_arg_watermark = ctx.body.local_decls.len();
        for arg in args {
            match &arg.node {
                ExpressionKind::NamedArgument(_name, value) => {
                    call_args.push(lower_expression(ctx, value, None)?);
                }
                _ => {
                    call_args.push(lower_expression(ctx, arg, None)?);
                }
            }
        }
        if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
            call_args.push(Operand::Copy(Place::new(alloc_local)));
        }

        let mangled_name = format!("{}_init", init_class);
        let func_op = Operand::Constant(Box::new(Constant {
            span: *span,
            ty: Type::new(TypeKind::Identifier, *span),
            literal: crate::ast::literal::Literal::Identifier(mangled_name),
        }));

        // init returns void; use a temp destination for the call
        let void_ty = Type::new(TypeKind::Void, *span);
        let void_dest = ctx.push_temp(void_ty, *span);
        let target_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: func_op,
                args: call_args.clone(),
                destination: Place::new(void_dest),
                target: Some(target_bb),
            },
            *span,
        ));
        ctx.set_current_block(target_bb);

        // Release managed temporaries created while lowering init call arguments.
        // Skip call_args[0] which is `self` (the destination, not a fresh temp).
        for arg_op in call_args.iter().skip(1) {
            if let Operand::Copy(place) | Operand::Move(place) = arg_op {
                ctx.emit_temp_drop(place.local, init_arg_watermark, *span);
            }
        }

        Ok(result_op)
    } else {
        // No init method anywhere in the chain — map constructor args directly to ALL fields.
        let arg_watermark = ctx.body.local_decls.len();
        let mut positional_args = Vec::with_capacity(args.len());
        let mut named_args: std::collections::HashMap<&str, Operand> =
            std::collections::HashMap::with_capacity(args.len());

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

        let mut operands = Vec::with_capacity(all_fields.len());
        let mut pos_iter = positional_args.into_iter();

        for (field_name, field_info) in &all_fields {
            let op = if let Some(op) = pos_iter.next() {
                op
            } else if let Some(op) = named_args.remove(field_name.as_str()) {
                op
            } else {
                create_default_value(&field_info.ty, span)
            };

            let op_ty = op.ty(&ctx.body).clone();
            let op = if op_ty.kind != field_info.ty.kind {
                let temp = ctx.push_temp(field_info.ty.clone(), *span);
                ctx.push_statement(crate::mir::Statement {
                    kind: StatementKind::Assign(
                        Place::new(temp),
                        coerce_rvalue(op, &op_ty, &field_info.ty),
                    ),
                    span: *span,
                });
                Operand::Copy(Place::new(temp))
            } else {
                op
            };

            operands.push(op);
        }

        let class_ty = Type::new(TypeKind::Custom(class_name.to_string(), None), *span);

        let (destination, result_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(class_ty.clone(), *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        };

        let dest_local = destination.local;
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                destination,
                Rvalue::Aggregate(AggregateKind::Class(class_ty), operands.clone()),
            ),
            span: *span,
        });

        // Release managed temporaries created while lowering the constructor arguments.
        for op in &operands {
            if let Operand::Copy(place) | Operand::Move(place) = op {
                if place.local != dest_local {
                    ctx.emit_temp_drop(place.local, arg_watermark, *span);
                }
            }
        }

        Ok(result_op)
    }
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

/// Computes the element size in bytes for a collection element type.
///
/// Primitives use their natural size. Managed types (String, collections,
/// custom types/classes) are pointer-sized since they are heap-allocated.
fn compute_elem_size_from_type(kind: &TypeKind) -> i64 {
    match kind {
        TypeKind::I8 | TypeKind::U8 | TypeKind::Boolean => 1,
        TypeKind::I16 | TypeKind::U16 => 2,
        TypeKind::I32 | TypeKind::U32 | TypeKind::F32 => 4,
        TypeKind::Int | TypeKind::I64 | TypeKind::U64 | TypeKind::Float | TypeKind::F64 => 8,
        TypeKind::I128 | TypeKind::U128 => 16,
        // All heap-allocated types are pointer-sized (8 bytes on 64-bit).
        // This includes String, List, Array, Map, Set, Custom (structs/enums/classes).
        TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Custom(_, _)
        | TypeKind::RawPtr => 8,
        // Default to 8 for unknown/complex types
        _ => 8,
    }
}
