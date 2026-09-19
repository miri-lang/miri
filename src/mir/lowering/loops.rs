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

use super::{lower_expression, lower_statement, LoweringContext};

pub fn lower_break(ctx: &mut LoweringContext, span: &Span) -> Result<(), LoweringError> {
    let (target, scope_depth) = match ctx.loop_stack.last() {
        Some(lc) => (lc.break_target, lc.scope_depth),
        None => return Err(LoweringError::break_outside_loop(*span)),
    };
    // Emit StorageDead for all named locals introduced inside the loop body
    // (scopes opened after enter_loop was called). Perceus converts these into
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

/// The variables a for-loop over an iterable binds.
struct LoopBindings {
    /// The element each pass reads.
    element: crate::mir::Local,
    /// The map value or list index, when the loop names a second variable.
    secondary: Option<crate::mir::Local>,
    is_map: bool,
    /// Bindings holding a reference-counted value. Each pass reads a fresh
    /// reference into them, so each pass owns and releases them.
    per_pass: Vec<crate::mir::Local>,
    /// Names of the `per_pass` bindings, which no scope unbinds on its own.
    per_pass_names: Vec<String>,
}

/// Bind the loop's element and, when declared, its second variable.
///
/// The second variable is bound first so an unmanaged one keeps the local it
/// has always had.
fn bind_loop_variables(
    ctx: &mut LoweringContext,
    span: &Span,
    decls: &[VariableDeclaration],
    iterable_ty: &Option<Type>,
) -> LoopBindings {
    let is_map = iterable_is_map(iterable_ty);
    let elem_ty = resolve_loop_elem_type(ctx, span, iterable_ty);
    let mut per_pass = Vec::new();
    let mut per_pass_names = Vec::new();
    let mut bind = |ctx: &mut LoweringContext, decl: &VariableDeclaration, ty: Type| {
        let is_managed = ctx.is_perceus_managed(&ty.kind);
        let local = setup_loop_variable(ctx, decl, ty, is_managed, span);
        if is_managed {
            per_pass.push(local);
            per_pass_names.push(decl.name.clone());
        }
        local
    };
    let secondary = match decls.get(1) {
        Some(decl) => {
            let ty = resolve_secondary_type(ctx, span, is_map, iterable_ty);
            Some(bind(ctx, decl, ty))
        }
        None => None,
    };
    let element = bind(ctx, &decls[0], elem_ty);
    LoopBindings {
        element,
        secondary,
        is_map,
        per_pass,
        per_pass_names,
    }
}

/// True when the iterable is a `Map` (normalized to `Custom("Map", ..)`).
fn iterable_is_map(iterable_ty: &Option<Type>) -> bool {
    match iterable_ty.as_ref().map(|t| &t.kind) {
        Some(TypeKind::Map(_, _)) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        Some(TypeKind::Custom(name, _))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
        {
            true
        }
        _ => false,
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
            super::resolve_type(ctx.type_checker, &elem_type_exprs[0])
        }
        TypeKind::Custom(name, Some(args))
            if (BuiltinCollectionKind::from_name(name).is_some()
                || name == crate::ast::types::TUPLE_TYPE_NAME)
                && !args.is_empty() =>
        {
            super::resolve_type(ctx.type_checker, &args[0])
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

/// The type of the second loop variable: a map's value, else a list index.
fn resolve_secondary_type(
    ctx: &mut LoweringContext,
    span: &Span,
    is_map: bool,
    iterable_ty: &Option<Type>,
) -> Type {
    if is_map {
        resolve_map_value_type(ctx, span, iterable_ty)
    } else {
        Type::new(TypeKind::Int, *span)
    }
}

/// Resolve a map's value type for the second loop variable, else default Int.
fn resolve_map_value_type(
    ctx: &mut LoweringContext,
    span: &Span,
    iterable_ty: &Option<Type>,
) -> Type {
    match iterable_ty.as_ref().map(|t| &t.kind) {
        Some(TypeKind::Map(_, _)) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        Some(TypeKind::Custom(name, Some(args)))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map)
                && args.len() == 2 =>
        {
            super::resolve_type(ctx.type_checker, &args[1])
        }
        _ => Type::new(TypeKind::Int, *span),
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

/// The class a `for` loop iterates through `length` and `element_at` calls,
/// or `None` when the iterable is indexed directly.
///
/// Iterability is decided by the type checker's own rule, so a loop the
/// checker typed is never lowered as a bare indexed walk — which would read the
/// object's own words as elements.
fn resolve_iterable_class(ctx: &LoweringContext, iterable_id: usize) -> Option<String> {
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
        .filter(|name| ctx.type_checker.class_is_iterable(name))
}

/// The class whose body a `for` loop over `class_name` calls for `method_name`.
///
/// A class that inherits `length` or `element_at` gets no copy of it: the body
/// belongs to the nearest ancestor that declares it and is compiled under that
/// ancestor's name. A class that declares or overrides the method answers for
/// itself, so the two are resolved separately — an override of one does not
/// move the other.
fn iterable_method_owner(ctx: &LoweringContext, class_name: &str, method_name: &str) -> String {
    crate::type_checker::context::class_method_declaration(
        class_name,
        method_name,
        ctx.type_checker.type_definitions(),
    )
    .map_or_else(|| class_name.to_string(), |(owner, _)| owner.to_string())
}

/// Emit length check and loop header condition.
/// Compute the iterable's length into `len_temp`: a `{Class}_length` runtime
/// call for collection classes, else a direct `Rvalue::Len`.
fn emit_loop_length(
    ctx: &mut LoweringContext,
    len_temp: crate::mir::Local,
    list_local: crate::mir::Local,
    iterable_class: &Option<String>,
    span: &Span,
) {
    let Some(class_name) = iterable_class else {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::Assign(Place::new(len_temp), Rvalue::Len(Place::new(list_local))),
            span: *span,
        });
        return;
    };
    let owner = iterable_method_owner(ctx, class_name, "length");
    let length_symbol = format!("{owner}_length");
    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier(length_symbol.clone()),
    }));
    let mut args = vec![Operand::Copy(Place::new(list_local))];
    if !length_symbol.starts_with("miri_") {
        if let Some(&allocator) = ctx.variable_map.get("allocator") {
            args.push(Operand::Copy(Place::new(allocator)));
        }
    }
    let after_len_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
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
    iterable_class: &Option<String>,
    idx_var: crate::mir::Local,
    idx_ty: &Type,
    body_bb: crate::mir::BasicBlock,
    exit_bb: crate::mir::BasicBlock,
    span: &Span,
) -> Result<(), LoweringError> {
    let len_temp = ctx.push_temp(idx_ty.clone(), *span);
    emit_loop_length(ctx, len_temp, list_local, iterable_class, span);

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
    class_name: &str,
    span: &Span,
) {
    let owner = iterable_method_owner(ctx, class_name, "element_at");
    let mut element_at_symbol = String::with_capacity(owner.len() + 11);
    element_at_symbol.push_str(&owner);
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
            arg_handles: Vec::new(),
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
                arg_handles: Vec::new(),
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

/// Load the pass's element, and its map value or list index when bound.
fn emit_loop_body_element_load(
    ctx: &mut LoweringContext,
    bindings: &LoopBindings,
    list_local: crate::mir::Local,
    idx_var: crate::mir::Local,
    iterable_class: &Option<String>,
    span: &Span,
) {
    let loop_var = bindings.element;
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

    if let Some(secondary) = bindings.secondary {
        emit_secondary_loop_var(ctx, secondary, list_local, idx_var, bindings.is_map, span);
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
    iterable_class: &Option<String>,
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
    emit_loop_body_element_load(ctx, bindings, list_local, idx_var, iterable_class, span);
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

    let iterable_ty = ctx.type_checker.get_type(iterable.id).cloned();
    let bindings = bind_loop_variables(ctx, span, decls, &iterable_ty);
    let list_local = lower_loop_iterable(ctx, iterable, iterable_ty, span)?;

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
    emit_loop_header(
        ctx,
        list_local,
        &iterable_class,
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
        &iterable_class,
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

/// Evaluate the loop's iterable into a temp the loop's scope owns, so the
/// reference is released when the loop ends — including by a `return` from
/// inside it.
fn lower_loop_iterable(
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
