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
        ctx.set_terminator(Terminator::new(TerminatorKind::Goto { target: join_bb }, span));
    }
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

/// Helper to extract element and secondary-variable types for a for-loop.
fn extract_loop_types(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable_ty: &Option<Type>,
) -> (Type, bool, Option<crate::mir::Local>) {
    let is_map = match iterable_ty.as_ref().map(|t| &t.kind) {
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
    let elem_ty = if let Some(ty) = iterable_ty {
        match &ty.kind {
            TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Tuple(elem_type_exprs) if !elem_type_exprs.is_empty() => {
                super::resolve_type(ctx.type_checker, &elem_type_exprs[0])
            }
            TypeKind::Custom(name, Some(args))
                if (BuiltinCollectionKind::from_name(name).is_some()
                    || name == crate::ast::types::TUPLE_TYPE_NAME)
                    && !args.is_empty() =>
            {
                super::resolve_type(ctx.type_checker, &args[0])
            }
            _ => ty.clone(),
        }
    } else {
        Type::new(TypeKind::Int, *span)
    };

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

    (elem_ty, is_map, idx_loop_var)
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
    }
}

/// Determine the iterable class and its method symbols if it implements Iterable.
fn resolve_iterable_class(
    ctx: &LoweringContext,
    iterable_id: usize,
) -> Option<String> {
    ctx.type_checker
        .get_type(iterable_id)
        .and_then(|ty| match &ty.kind {
            TypeKind::String => Some(crate::ast::types::STRING_TYPE_NAME.to_string()),
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
            TypeKind::Custom(name, _)
                if !matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) && name != crate::ast::types::TUPLE_TYPE_NAME =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .filter(|name| {
            matches!(
                ctx.type_checker.global_type_definitions.get(name),
                Some(TypeDefinition::Class(class_def)) if class_def.traits.iter().any(|t| t == "Iterable")
            )
        })
}

/// Emit length check and loop header condition.
#[allow(clippy::too_many_arguments)]
fn emit_loop_header(
    ctx: &mut LoweringContext,
    list_local: crate::mir::Local,
    iterable_class: &Option<String>,
    idx_var: crate::mir::Local,
    idx_ty: &Type,
    body_bb: crate::mir::BasicBlock,
    exit_bb: crate::mir::BasicBlock,
    span: &Span,
) -> Result<(), LoweringError> {
    let len_temp = ctx.push_temp(idx_ty.clone(), *span);

    if let Some(ref class_name) = iterable_class {
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

    Ok(())
}

/// Emit element load and secondary variable assignment within loop body.
/// Emit element load from collection within loop body.
fn emit_element_at_call(
    ctx: &mut LoweringContext,
    loop_var: crate::mir::Local,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    class_name: &str,
    span: &Span,
) {
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
}

/// Emit secondary loop variable assignment (map value or list index).
fn emit_secondary_loop_var(
    ctx: &mut LoweringContext,
    idx_local: crate::mir::Local,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    is_map: bool,
    span: &Span,
) {
    if is_map {
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
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(
                Place::new(idx_local),
                Rvalue::Use(Operand::Copy(Place::new(idx_var))),
            ),
            span: *span,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_loop_body_element_load(
    ctx: &mut LoweringContext,
    loop_var: crate::mir::Local,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    iterable_class: &Option<String>,
    elem_is_managed: bool,
    idx_loop_var: Option<crate::mir::Local>,
    is_map: bool,
    span: &Span,
) -> Result<(), LoweringError> {
    if elem_is_managed {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageLive(Place::new(loop_var)),
            span: *span,
        });
    }

    if let Some(ref class_name) = iterable_class {
        emit_element_at_call(ctx, loop_var, list_local, idx_var, class_name, span);
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

    if let Some(idx_local) = idx_loop_var {
        emit_secondary_loop_var(ctx, idx_local, list_local, idx_var, is_map, span);
    }

    Ok(())
}

/// Emit element cleanup and index increment at loop increment block.
fn emit_loop_increment(
    ctx: &mut LoweringContext,
    loop_var: crate::mir::Local,
    elem_is_managed: bool,
    idx_var: crate::mir::Local,
    idx_ty: &Type,
    header_bb: crate::mir::BasicBlock,
    span: &Span,
) {
    if elem_is_managed {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageDead(Place::new(loop_var)),
            span: *span,
        });
    }

    let one = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: idx_ty.clone(),
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
    ctx.push_scope();

    let decl = &decls[0];
    let iterable_ty = ctx.type_checker.get_type(iterable.id).cloned();
    let (elem_ty, is_map, idx_loop_var) = extract_loop_types(ctx, span, decls, &iterable_ty);
    let elem_is_managed = ctx.is_perceus_managed(&elem_ty.kind);

    let loop_var = setup_loop_variable(ctx, decl, elem_ty, elem_is_managed, span);

    let list_ty = if let Some(ty) = ctx.type_checker.get_type(iterable.id) {
        ty.clone()
    } else {
        Type::new(TypeKind::Void, *span)
    };
    let list_local = ctx.push_temp(list_ty, *span);
    lower_expression(ctx, iterable, Some(Place::new(list_local)))?;

    let idx_ty = Type::new(TypeKind::Int, *span);
    let idx_var = ctx.push_temp(idx_ty.clone(), *span);

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

    let iterable_class = resolve_iterable_class(ctx, iterable.id);

    ctx.set_current_block(header_bb);
    emit_loop_header(ctx, list_local, &iterable_class, idx_var, &idx_ty, body_bb, exit_bb, span)?;

    ctx.enter_loop(exit_bb, increment_bb);
    ctx.set_current_block(body_bb);
    emit_loop_body_element_load(
        ctx,
        loop_var,
        list_local,
        idx_var,
        &iterable_class,
        elem_is_managed,
        idx_loop_var,
        is_map,
        span,
    )?;

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

    ctx.set_current_block(increment_bb);
    emit_loop_increment(ctx, loop_var, elem_is_managed, idx_var, &idx_ty, header_bb, span);

    ctx.set_current_block(exit_bb);
    ctx.emit_temp_drop(list_local, 0, *span);
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
