// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Loop and control-flow lowering — if, while, for, break, continue.

use std::rc::Rc;

use crate::ast::expression::Expression;
use crate::ast::statement::{IfStatementType, Statement};
use crate::ast::{
    BuiltinCollectionKind, ExpressionKind, RangeExpressionType, Type, TypeKind,
    VariableDeclaration, WhileStatementType,
};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BinOp, Constant, Discriminant, Operand, Place, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use crate::type_checker::context::TypeDefinition;

use super::{lower_expression, lower_statement, LoweringContext};

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
        // Canonical variants are normalized to Custom before MIR lowering.
        Some(TypeKind::Map(_, _)) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        Some(TypeKind::Custom(name, _))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
        {
            true
        }
        _ => false,
    };
    let elem_ty = if let Some(ty) = &iterable_ty {
        // Extract element type from collection type parameters.
        // After normalization, all builtin collections are Custom("Name", args).
        match &ty.kind {
            TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Tuple(elem_type_exprs) if !elem_type_exprs.is_empty() => {
                super::resolve_type(ctx.type_checker, &elem_type_exprs[0])
            }
            TypeKind::Custom(name, Some(args))
                if (BuiltinCollectionKind::from_name(name).is_some() || name == "Tuple")
                    && !args.is_empty() =>
            {
                super::resolve_type(ctx.type_checker, &args[0])
            }
            _ => ty.clone(),
        }
    } else {
        Type::new(TypeKind::Int, *span)
    };

    // Determine if the element type needs RC management.
    let elem_is_managed = ctx.is_perceus_managed(&elem_ty.kind);

    // For managed element types we must emit StorageLive at the *start* of each body
    // iteration and StorageDead at the *start* of the increment block.  This causes
    // Perceus to insert a DecRef for the previous iteration's element before the next
    // element_at overwrites loop_var, preventing a reference-count leak of N-1 elements.
    //
    // To achieve this without pop_scope emitting a redundant StorageDead for loop_var
    // (which would DecRef an uninitialized slot on the empty-list path), we create the
    // local via push_temp (which does NOT add it to scope.introduced) and manually
    // register it in variable_map so that the loop body can resolve it by name.
    //
    // For non-managed element types (e.g. Int), use the regular push_local path.
    let loop_var = if elem_is_managed {
        let local = ctx.push_temp(elem_ty.clone(), *span);
        if !ctx.is_release {
            ctx.body.local_decls[local.0].name = Some(Rc::from(decl.name.as_str()));
        }
        ctx.body.local_decls[local.0].is_user_variable = true;
        // Register in variable_map, saving any shadowed binding so pop_scope can restore it.
        let name_rc: Rc<str> = Rc::from(decl.name.as_str());
        match ctx.variable_map.entry(name_rc) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old_local = *entry.get();
                if let Some(scope) = ctx.scope_stack.last_mut() {
                    scope.shadowed.insert(entry.key().clone(), old_local);
                }
                entry.insert(local);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(local);
            }
        }
        local
    } else {
        ctx.push_local(decl.name.clone(), elem_ty, *span)
    };

    // If there's a second declaration:
    // - For Map: it's the value variable (for k, v in map)
    // - For others: it's the index variable (for val, idx in list)
    let idx_loop_var = if decls.len() > 1 {
        let idx_decl = &decls[1];
        let var_ty = if is_map {
            match iterable_ty.as_ref().map(|t| &t.kind) {
                Some(TypeKind::Map(_, _)) => {
                    unreachable!("collection types are normalized to Custom before this point")
                }
                Some(TypeKind::Custom(name, Some(args)))
                    if BuiltinCollectionKind::from_name(name)
                        == Some(BuiltinCollectionKind::Map)
                        && args.len() == 2 =>
                {
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
            // Canonical variants are normalized to Custom before MIR lowering.
            TypeKind::Map(_, _) | TypeKind::Set(_) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(name, _)
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Map | BuiltinCollectionKind::Set)
                ) =>
            {
                Some(name.clone())
            }
            TypeKind::Custom(name, _) if !matches!(BuiltinCollectionKind::from_name(name), Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)) && name != "Tuple" => Some(name.clone()),
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
        let mut length_symbol = String::with_capacity(class_name.len() + 7);
        length_symbol.push_str(class_name);
        length_symbol.push_str("_length");
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
                out_args: Vec::new(),
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

    // For managed element types, start the lifetime of loop_var at the top of each
    // body iteration.  Perceus will pair this StorageLive with the StorageDead in the
    // increment block, yielding an IncRef (for the element_at Copy below) followed by
    // a DecRef (when the iteration ends), correctly balancing every element reference.
    if elem_is_managed {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageLive(Place::new(loop_var)),
            span: *span,
        });
    }

    // Assign loop_var = element_at(idx) or list[idx]
    if let Some(ref class_name) = iterable_class {
        // Call ClassName_element_at(iterable, idx) via MIR terminator
        let mut element_at_symbol = String::with_capacity(class_name.len() + 11);
        element_at_symbol.push_str(class_name);
        element_at_symbol.push_str("_element_at");
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
                out_args: Vec::new(),
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
                    out_args: Vec::new(),
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

    // For managed element types, end the lifetime of loop_var at the top of the
    // increment block.  Perceus converts this StorageDead into a DecRef, releasing
    // the current iteration's element reference before the next element_at is called.
    if elem_is_managed {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageDead(Place::new(loop_var)),
            span: *span,
        });
    }

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
    // For managed loop vars, remove the manually-registered binding before pop_scope.
    // pop_scope's shadowed.drain() will restore any outer binding that was shadowed.
    if elem_is_managed {
        ctx.variable_map.remove(decl.name.as_str());
    }
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
