// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Helper functions for MIR lowering.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::Pattern;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    Discriminant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};
use crate::type_checker::TypeChecker;

use super::context::LoweringContext;
use super::expression::lower_expression;
use super::statement::lower_statement;

/// Ensure an operand is materialized as a `Place`.
///
/// If the operand is already a `Copy` or `Move` of a place, returns it directly.
/// If the operand is a `Constant`, stores it in a fresh temp local and returns
/// that temp's place.
pub fn ensure_place(ctx: &mut LoweringContext, operand: Operand, span: Span) -> Place {
    match operand {
        Operand::Copy(p) | Operand::Move(p) => p,
        Operand::Constant(c) => {
            let temp = ctx.push_temp(c.ty.clone(), span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(Operand::Constant(c))),
                span,
            });
            Place::new(temp)
        }
    }
}

/// Resolve an AST type expression to a concrete `Type`.
///
/// This function attempts to resolve type expressions in the following order:
/// 1. Look up the expression ID in the type checker's type map
/// 2. Parse the expression structure directly (Type nodes, Identifiers)
///
/// # Returns
/// The resolved type. If resolution fails, returns `TypeKind::Error` to allow
/// graceful error propagation rather than panicking.
///
/// # Note
/// Unknown types produce `TypeKind::Error` instead of panicking. Callers should
/// check for this and report appropriate errors if needed.
pub fn resolve_type(tc: &TypeChecker, expr: &Expression) -> Type {
    if let Some(ty) = tc.get_type(expr.id) {
        return ty.clone();
    }

    match &expr.node {
        ExpressionKind::Type(t, is_nullable) => {
            if *is_nullable {
                Type::new(TypeKind::Nullable(t.clone()), expr.span)
            } else {
                *t.clone()
            }
        }
        ExpressionKind::Identifier(name, _) => {
            if tc.global_type_definitions.contains_key(name) {
                Type::new(TypeKind::Custom(name.clone(), None), expr.span)
            } else {
                match name.as_str() {
                    "int" => Type::new(TypeKind::Int, expr.span),
                    "bool" => Type::new(TypeKind::Boolean, expr.span),
                    "string" => Type::new(TypeKind::String, expr.span),
                    "float" => Type::new(TypeKind::Float, expr.span),
                    "void" => Type::new(TypeKind::Void, expr.span),
                    // Fallback: Unknown primitive type - use Error type instead of panicking
                    _ => Type::new(TypeKind::Error, expr.span),
                }
            }
        }
        // Fallback: Unsupported type expression - use Error type instead of panicking
        _ => Type::new(TypeKind::Error, expr.span),
    }
}

/// Convert a literal to u128 for SwitchInt discrimination.
/// For signed integers, we reinterpret as unsigned to preserve bit patterns,
/// then extend to u128. This ensures -1i8 becomes 255 (0xFF), not u128::MAX.
pub fn literal_to_u128(lit: &crate::ast::literal::Literal) -> Option<u128> {
    use crate::ast::literal::{IntegerLiteral, Literal};
    match lit {
        Literal::Integer(int_lit) => match int_lit {
            // Signed: reinterpret bits as unsigned first, then zero-extend to u128
            IntegerLiteral::I8(v) => Some((*v as u8) as u128),
            IntegerLiteral::I16(v) => Some((*v as u16) as u128),
            IntegerLiteral::I32(v) => Some((*v as u32) as u128),
            IntegerLiteral::I64(v) => Some((*v as u64) as u128),
            IntegerLiteral::I128(v) => Some(*v as u128),
            // Unsigned: direct conversion
            IntegerLiteral::U8(v) => Some(*v as u128),
            IntegerLiteral::U16(v) => Some(*v as u128),
            IntegerLiteral::U32(v) => Some(*v as u128),
            IntegerLiteral::U64(v) => Some(*v as u128),
            IntegerLiteral::U128(v) => Some(*v),
        },
        Literal::Boolean(b) => Some(if *b { 1 } else { 0 }),
        // String, Float, Symbol - can't be used with SwitchInt directly
        _ => None,
    }
}

/// Bind pattern variables to the subject value.
pub fn bind_pattern(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject_local: crate::mir::Local,
    span: &crate::error::syntax::Span,
) -> Result<(), LoweringError> {
    match pattern {
        Pattern::Identifier(name) => {
            // Create a new local for the bound variable
            let ty = ctx.body.local_decls[subject_local.0].ty.clone();
            let var_local = ctx.push_local(name.clone(), ty, *span);

            // Assign subject value to bound variable
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(var_local),
                    Rvalue::Use(Operand::Copy(Place::new(subject_local))),
                ),
                span: *span,
            });
        }
        Pattern::Tuple(patterns) => {
            // For tuple destructuring, create bindings for each element
            // Tuple fields are statically known, so we use Field projection

            // Extract element types from the tuple type definition
            let tuple_ty = ctx.body.local_decls[subject_local.0].ty.clone();
            let element_types: Vec<Type> = if let TypeKind::Tuple(elems) = &tuple_ty.kind {
                elems
                    .iter()
                    .map(|e| resolve_type(ctx.type_checker, e))
                    .collect()
            } else {
                // Fallback: use the whole tuple type (should not happen after type checking)
                vec![tuple_ty.clone(); patterns.len()]
            };

            for (i, p) in patterns.iter().enumerate() {
                if let Pattern::Identifier(name) = p {
                    let elem_ty = element_types
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
                    let elem_local = ctx.push_local(name.clone(), elem_ty, *span);

                    // Create Field projection for tuple element (static index)
                    let mut place = Place::new(subject_local);
                    place.projection.push(PlaceElem::Field(i));

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(elem_local),
                            Rvalue::Use(Operand::Copy(place)),
                        ),
                        span: *span,
                    });
                }
            }
        }
        Pattern::EnumVariant(parent, bindings) => {
            // For enum variant destructuring, extract associated values.
            // The aggregate is (discriminant, val1, val2, ...), so bindings use Field(i+1).

            // Resolve the concrete field types from the enum definition so that the
            // bound locals are typed correctly (e.g. `int` instead of `void`).
            let field_types: Option<Vec<Type>> =
                if let Pattern::Member(type_pattern, variant_name) = parent.as_ref() {
                    if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                        if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                            ctx.type_checker.global_type_definitions.get(type_name)
                        {
                            enum_def.variants.get(variant_name.as_str()).cloned()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

            for (i, binding) in bindings.iter().enumerate() {
                if let Pattern::Identifier(name) = binding {
                    // Use the actual field type from the enum definition; fall back to
                    // Void only if the definition cannot be resolved (should not happen
                    // after a successful type check).
                    let ty = field_types
                        .as_ref()
                        .and_then(|types| types.get(i))
                        .cloned()
                        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
                    let elem_local = ctx.push_local(name.clone(), ty, *span);

                    // Create Field projection for element (i+1 to skip discriminant at field 0)
                    let mut place = Place::new(subject_local);
                    place.projection.push(PlaceElem::Field(i + 1));

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(elem_local),
                            Rvalue::Use(Operand::Copy(place)),
                        ),
                        span: *span,
                    });
                }
            }
        }
        // Literal, Default, Regex, Member - no bindings needed
        _ => {}
    }
    Ok(())
}

/// Helper to lower a statement and assign the result expression to a target local.
/// This is used for match branches where each branch result should be assigned to result_local.
pub fn lower_to_local(
    ctx: &mut LoweringContext,
    stmt: &Statement,
    target_local: crate::mir::Local,
    result_ty: &Type,
) -> Result<(), LoweringError> {
    if matches!(result_ty.kind, TypeKind::Void) {
        lower_statement(ctx, stmt)?;
        return Ok(());
    }

    match &stmt.node {
        StatementKind::Expression(expr) => {
            let operand = lower_expression(ctx, expr, None)?;
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(target_local), Rvalue::Use(operand)),
                span: expr.span,
            });
        }
        StatementKind::Block(stmts) => {
            ctx.push_scope();
            let last_meaningful_idx = stmts
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| !matches!(&s.node, StatementKind::Block(inner) if inner.is_empty()))
                .map(|(i, _)| i);

            for (i, s) in stmts.iter().enumerate() {
                if Some(i) == last_meaningful_idx {
                    lower_to_local(ctx, s, target_local, result_ty)?;
                } else {
                    lower_statement(ctx, s)?;
                }
            }
            ctx.pop_scope(stmt.span);
        }
        _ => lower_statement(ctx, stmt)?,
    }
    Ok(())
}

/// Recursively lowers statements to assign the final expression to `_0` (return place).
pub fn lower_as_return(
    ctx: &mut LoweringContext,
    stmt: &Statement,
    ret_ty: &Type,
) -> Result<(), LoweringError> {
    if matches!(ret_ty.kind, TypeKind::Void) {
        lower_statement(ctx, stmt)?;
        return Ok(());
    }

    match &stmt.node {
        StatementKind::Expression(expr) => {
            let operand = lower_expression(ctx, expr, None)?;
            let op_ty = operand.ty(&ctx.body);

            let rvalue = if op_ty.kind != ret_ty.kind {
                Rvalue::Cast(Box::new(operand), ret_ty.clone())
            } else {
                Rvalue::Use(operand)
            };

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(crate::mir::Local(0)), rvalue),
                span: expr.span,
            });
        }
        StatementKind::Block(stmts) => {
            ctx.push_scope();

            // Find the index of the last non-empty statement for return value
            // (skip trailing empty blocks which can be created by trailing whitespace)
            let last_meaningful_idx = stmts
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| !matches!(&s.node, StatementKind::Block(inner) if inner.is_empty()))
                .map(|(i, _)| i);

            for (i, s) in stmts.iter().enumerate() {
                if Some(i) == last_meaningful_idx {
                    lower_as_return(ctx, s, ret_ty)?;
                } else {
                    lower_statement(ctx, s)?;
                }
            }
            ctx.pop_scope(stmt.span);
        }
        StatementKind::If(cond, then_stmt, else_stmt, if_type) => {
            let cond_op = lower_expression(ctx, cond, None)?;
            let then_bb = ctx.new_basic_block();
            let else_bb = ctx.new_basic_block();
            let join_bb = ctx.new_basic_block();

            let (target_val, other_target) = match if_type {
                crate::ast::statement::IfStatementType::If => (1, else_bb),
                crate::ast::statement::IfStatementType::Unless => (0, else_bb),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Discriminant::from(target_val), then_bb)],
                    otherwise: other_target,
                },
                stmt.span,
            ));

            // Lower Then
            ctx.set_current_block(then_bb);
            lower_as_return(ctx, then_stmt, ret_ty)?;
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: join_bb },
                    stmt.span,
                ));
            }

            // Lower Else
            ctx.set_current_block(else_bb);
            if let Some(else_s) = else_stmt {
                lower_as_return(ctx, else_s, ret_ty)?;
            }
            if ctx.body.basic_blocks[ctx.current_block.0]
                .terminator
                .is_none()
            {
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Goto { target: join_bb },
                    stmt.span,
                ));
            }
            ctx.set_current_block(join_bb);
        }
        _ => lower_statement(ctx, stmt)?,
    }
    Ok(())
}
