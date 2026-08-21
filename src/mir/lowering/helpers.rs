// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Helper functions for MIR lowering.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::Pattern;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::types::MirType;
use crate::mir::{
    Discriminant, ExecutionModel, Operand, Place, PlaceElem, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
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
    // Type-wrapper expressions (`ExpressionKind::Type`) carry their resolved
    // type directly and are routinely synthesized with id=0. Reading the
    // type-checker cache by id collides with any other id=0 expression that
    // happens to have been stored last, so we trust the inner type instead.
    if let ExpressionKind::Type(t, is_nullable) = &expr.node {
        if *is_nullable {
            return Type::new(TypeKind::Option(t.clone()), expr.span);
        }
        return *t.clone();
    }

    // Tripwire: any non-Type synthesized expression that reaches the cache
    // with id=0 will collide with other id=0 entries (same hazard the Type
    // short-circuit avoids). If this fires, allocate a real id at the
    // synthesis site via `expr_with_span` instead of constructing
    // `Expression` directly with `id: 0`.
    debug_assert!(
        expr.id != 0,
        "expression with id=0 hit the MIR type cache — synthesizer missed an id"
    );

    if let Some(ty) = tc.get_type(expr.id) {
        return ty.clone();
    }

    match &expr.node {
        ExpressionKind::Type(t, is_nullable) => {
            if *is_nullable {
                Type::new(TypeKind::Option(t.clone()), expr.span)
            } else {
                *t.clone()
            }
        }
        ExpressionKind::Identifier(name, _) => {
            if tc.type_definitions().contains_key(name) {
                Type::new(TypeKind::Custom(name.clone(), None), expr.span)
            } else {
                match name.as_str() {
                    "int" => Type::new(TypeKind::Int, expr.span),
                    "bool" => Type::new(TypeKind::Boolean, expr.span),
                    s if s == crate::ast::types::STRING_TYPE_NAME => {
                        Type::new(TypeKind::String, expr.span)
                    }
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
        // String, Float, Identifier - can't be used with SwitchInt directly
        _ => None,
    }
}

/// Whether a pattern name discards its match rather than naming it.
///
/// A discard binds nothing: giving it a local would put several same-named
/// entries in one scope, and only the last of them would ever be released.
fn is_discard(name: &str) -> bool {
    name == "_"
}

/// Bind pattern variables to the subject value.
pub fn bind_pattern(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject_local: crate::mir::Local,
    span: &crate::error::syntax::Span,
) -> Result<(), LoweringError> {
    match pattern {
        Pattern::Identifier(name) if !is_discard(name) => {
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
                    if is_discard(name) {
                        continue;
                    }
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
            // Handle Option Some(x) pattern — bind subject directly (identity, like unwrap)
            let is_option_some = {
                let subject_ty = &ctx.body.local_decls[subject_local.0].ty;
                if matches!(subject_ty.kind, TypeKind::Option(_)) {
                    match parent.as_ref() {
                        Pattern::Identifier(name) => name == "Some",
                        Pattern::Member(enum_pat, variant) => {
                            matches!(
                                enum_pat.as_ref(),
                                Pattern::Identifier(n) if n == crate::ast::types::OPTION_TYPE_NAME
                            ) && variant == "Some"
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            };
            if is_option_some {
                if let Some(Pattern::Identifier(name)) = bindings
                    .first()
                    .filter(|first| !matches!(first, Pattern::Identifier(n) if is_discard(n)))
                {
                    let subject_ty = ctx.body.local_decls[subject_local.0].ty.clone();
                    // The inner type is the boxed value
                    let inner_ty = if let TypeKind::Option(inner) = &subject_ty.kind {
                        inner.as_ref().clone()
                    } else {
                        subject_ty
                    };
                    let var_local = ctx.push_local(name.clone(), inner_ty.clone(), *span);
                    let mut place = Place::new(subject_local);
                    place.projection.push(PlaceElem::Field(0));
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(var_local),
                            Rvalue::Use(Operand::Copy(place)),
                        ),
                        span: *span,
                    });
                    // Perceus now handles Field(0) projections on Option types via
                    // is_place_managed, so no explicit IncRef needed here.
                }
                return Ok(());
            }

            // For enum variant destructuring, extract associated values.
            // The aggregate is (discriminant, val1, val2, ...), so bindings use Field(i+1).

            let field_types = variant_payload_types(ctx, parent, subject_local);

            for (i, binding) in bindings.iter().enumerate() {
                if let Pattern::Identifier(name) = binding {
                    if is_discard(name) {
                        continue;
                    }
                    // Use the actual field type from the enum definition; fall back to
                    // Void only if the definition cannot be resolved (should not happen
                    // after a successful type check).
                    let ty = field_types
                        .as_ref()
                        .and_then(|types| types.get(i))
                        .cloned()
                        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
                    let elem_local = ctx.push_local(name.clone(), ty.clone(), *span);

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
                    // Perceus handles enum field projections via the managed_locals fallback
                    // in its main loop, so no explicit IncRef needed here.
                }
            }
        }
        // Literal, Default, Regex, Member - no bindings needed
        _ => {}
    }
    Ok(())
}

/// The payload types a variant pattern binds, so the bound locals are typed
/// from the enum definition (e.g. `int` rather than `void`) instead of
/// defaulting to a pointer slot.
///
/// A generic enum spells its payloads as type parameters, so they are resolved
/// through the subject's instantiation arguments — otherwise a `Result<f64, E>`
/// payload would be typed `T` and read back at pointer width instead of the
/// float width it was stored at. Returns `None` when the pattern does not name
/// a known enum variant.
fn variant_payload_types(
    ctx: &LoweringContext,
    parent: &Pattern,
    subject_local: crate::mir::Local,
) -> Option<Vec<Type>> {
    let Pattern::Member(type_pattern, variant_name) = parent else {
        return None;
    };
    let Pattern::Identifier(type_name) = type_pattern.as_ref() else {
        return None;
    };
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(type_name)
    else {
        return None;
    };
    let declared = enum_def.variants.get(variant_name.as_str())?;
    let type_args = enum_instantiation_args(&ctx.body.local_decls[subject_local.0].ty.kind);
    Some(substitute_variant_field_types(
        declared,
        type_args.as_deref(),
        enum_def,
    ))
}

/// The type arguments of an enum instantiation, in the order the enum declares
/// its parameters.
///
/// The type checker normalizes `Result<T, E>` to `TypeKind::Custom`, but the
/// dedicated `TypeKind::Result` spelling also reaches lowering, so both forms
/// are recognized. Any other type carries no enum instantiation arguments.
fn enum_instantiation_args(kind: &TypeKind) -> Option<Vec<Expression>> {
    if let TypeKind::Custom(_, Some(args)) = kind {
        Some(args.clone())
    } else if let TypeKind::Result(ok_ty, err_ty) = kind {
        Some(vec![(**ok_ty).clone(), (**err_ty).clone()])
    } else {
        None
    }
}

/// Resolve a variant's declared payload types against an instantiation's type
/// arguments, so a payload spelled as a type parameter takes the concrete type
/// bound at the match site. A payload that stays generic (no arguments known,
/// as inside a generic function body) is left alone and keeps its pointer-sized
/// representation.
///
/// Substitution recovers a payload's *storage width*, which only differs from
/// the pointer-sized default for scalars, and its *ownership*: a payload named
/// concretely as a reference-counted type makes the bound local managed, so
/// Perceus retains it at the bind. That retain is what balances the release the
/// enum's drop path emits for the same field once it resolves the instantiation
/// (`codegen::cranelift::rc::enum_variants_with_managed_fields`). Both sides
/// read the same instantiation arguments, so a payload is managed at the bind
/// exactly when it is released at the drop; substituting on only one side
/// leaks (no release) or double-frees (release without retain).
fn substitute_variant_field_types(
    declared: &[Type],
    type_args: Option<&[Expression]>,
    enum_def: &crate::type_checker::context::EnumDefinition,
) -> Vec<Type> {
    declared
        .iter()
        .map(|ty| {
            let substituted = crate::type_checker::generics::substitute_generic_field_kind(
                &ty.kind,
                type_args,
                enum_def.generics.as_ref(),
            );
            Type::new(substituted, ty.span)
        })
        .collect()
}

/// Returns true when two MirTypes have the same outer constructor, ignoring inner type args.
///
/// Used in `lower_as_return` to detect structurally-compatible generic collection types.
/// For example, `List(Generic)` vs `List(Custom("T"))` both represent list pointers and
/// are safe to assign via DPS without an intermediate Cast.
pub(crate) fn mir_types_structurally_match(a: &MirType, b: &MirType) -> bool {
    matches!(
        (a, b),
        (MirType::List(_), MirType::List(_))
            | (MirType::Map(_, _), MirType::Map(_, _))
            | (MirType::Set(_), MirType::Set(_))
            | (MirType::Array(_), MirType::Array(_))
            | (MirType::Option(_), MirType::Option(_))
            | (MirType::Result(_, _), MirType::Result(_, _))
            | (MirType::Tuple(_), MirType::Tuple(_))
            | (MirType::Future(_), MirType::Future(_))
    )
}

/// Whether two types are the same value at the MIR level, differing only in how
/// their type is spelled.
///
/// `Array<int, 3>` and `Array<int, SIZE>` are one array; `List(T)` and
/// `List(Custom("T"))` are one list. Coercing between them would hand the same
/// object a second holder while both holders still get released, so the pair is
/// passed through untouched instead.
pub fn spellings_of_one_value(from_ty: &Type, to_ty: &Type) -> bool {
    let from = MirType::from_type_kind(&from_ty.kind);
    let to = MirType::from_type_kind(&to_ty.kind);
    from == to || mir_types_structurally_match(&from, &to)
}

/// Whether coercing `op_ty` into `target_ty` hands the value a second holder.
///
/// Wrapping a bare `T` into an `Option` builds an aggregate, and Perceus retains
/// every managed place an aggregate reads. A value a callee has just donated
/// lives in a temp no scope releases, so that retain has to be answered.
pub fn coercion_retains_source(op_ty: &Type, target_ty: &Type) -> bool {
    matches!(target_ty.kind, TypeKind::Option(_)) && !matches!(op_ty.kind, TypeKind::Option(_))
}

/// Release the temp a retaining coercion read, when the expression being lowered
/// is what created it.
///
/// `emit_temp_drop` leaves named locals, borrowed temps and scope-owned locals
/// untouched, so a value some other holder still owns keeps its reference.
pub fn release_coerced_source(
    ctx: &mut LoweringContext,
    operand: &Operand,
    op_ty: &Type,
    target_ty: &Type,
    watermark: usize,
    span: Span,
) {
    if !coercion_retains_source(op_ty, target_ty) {
        return;
    }
    if let Operand::Copy(place) | Operand::Move(place) = operand {
        ctx.emit_temp_drop(place.local, watermark, span);
    }
}

/// Helper to construct an Rvalue that coerces `operand` of type `op_ty` into `target_ty`.
/// If `target_ty` is `Option<T>` and `op_ty` is `T`, it allocates an Option box.
/// Otherwise, it emits a standard type Cast.
pub fn coerce_rvalue(operand: Operand, op_ty: &Type, target_ty: &Type) -> Rvalue {
    if matches!(target_ty.kind, TypeKind::Option(_)) && !matches!(op_ty.kind, TypeKind::Option(_)) {
        crate::mir::Rvalue::Aggregate(crate::mir::AggregateKind::Option, vec![operand])
    } else {
        crate::mir::Rvalue::Cast(Box::new(operand), target_ty.clone())
    }
}

/// Helper to lower a statement and assign the result expression to a target local.
/// This is used for match branches where each branch result should be assigned to result_local.
/// Lower an expression statement and copy its value into `target_local`,
/// dropping the source temp afterwards (the Copy/Use IncRef leaves both owning).
fn assign_expr_to_local(
    ctx: &mut LoweringContext,
    expr: &Expression,
    target_local: crate::mir::Local,
) -> Result<(), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let lowered = lower_expression(ctx, expr, None)?;
    // A local that already existed before this expression — a match-arm binding
    // or an enclosing variable — is released when its scope ends, and moving out
    // of it does not cancel that release. Read it by copy so the value handed to
    // `target_local` is retained rather than freed out from under the target.
    let operand = match lowered {
        Operand::Move(place) if place.local.0 < watermark && place.projection.is_empty() => {
            Operand::Copy(place)
        }
        already_usable => already_usable,
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(target_local), Rvalue::Use(operand.clone())),
        span: expr.span,
    });
    // Reading a field out of a freshly built aggregate leaves the aggregate
    // itself with no other reference, so the base local is dropped even when the
    // value taken from it is a projection. `emit_temp_drop` still refuses to
    // touch anything older than the watermark, which is what keeps a match-arm
    // binding or an enclosing variable from being released here.
    if let Operand::Copy(p) | Operand::Move(p) = &operand {
        if p.local != target_local {
            ctx.emit_temp_drop(p.local, watermark, expr.span);
        }
    }
    Ok(())
}

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
        StatementKind::Expression(expr) => assign_expr_to_local(ctx, expr, target_local)?,
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
            let expr_ty = ctx.type_checker.get_type(expr.id).cloned();
            let types_match = expr_ty
                .as_ref()
                .map(|t| {
                    let em = MirType::from_type_kind(&t.kind);
                    let rm = MirType::from_type_kind(&ret_ty.kind);
                    // Exact match OR structurally-equivalent outer type.  The latter
                    // handles generic collection returns where element types differ
                    // (e.g. MirType::List(Generic) vs MirType::List(Custom("T"))):
                    // both sides are compatible pointer-sized values, so DPS is safe.
                    em == rm || mir_types_structurally_match(&em, &rm)
                })
                .unwrap_or(false);

            if types_match {
                // DPS: write directly to _0 to avoid a temp that would leak.
                lower_expression(ctx, expr, Some(Place::new(crate::mir::Local(0))))?;
            } else {
                let watermark = ctx.body.local_decls.len();
                let operand = lower_expression(ctx, expr, None)?;
                let op_ty = operand.ty(&ctx.body).clone();
                let rvalue = coerce_rvalue(operand.clone(), &op_ty, ret_ty);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(Place::new(crate::mir::Local(0)), rvalue),
                    span: expr.span,
                });
                // Drop any managed temp created during the expression.
                if let Operand::Copy(place) | Operand::Move(place) = &operand {
                    ctx.emit_temp_drop(place.local, watermark, expr.span);
                }
            }
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
            // Coerce Option condition to bool
            let cond_op = if let Some(cond_ty) = ctx.type_checker.get_type(cond.id) {
                if let TypeKind::Option(inner_ty) = &cond_ty.kind {
                    let none_val = Operand::Constant(Box::new(crate::mir::Constant {
                        span: stmt.span,
                        ty: inner_ty.as_ref().clone(),
                        literal: crate::ast::literal::Literal::None,
                    }));
                    let bool_ty = Type::new(TypeKind::Boolean, stmt.span);
                    let temp = ctx.push_temp(bool_ty, stmt.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::BinaryOp(
                                crate::mir::BinOp::Ne,
                                Box::new(cond_op),
                                Box::new(none_val),
                            ),
                        ),
                        span: stmt.span,
                    });
                    Operand::Copy(Place::new(temp))
                } else {
                    cond_op
                }
            } else {
                cond_op
            };
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

/// Adjust math intrinsic return type for GPU kernels.
///
/// In GPU kernel execution, math intrinsics that take f32 arguments must produce
/// f32 results, not f64. This prevents buffer width mismatches (e.g., sqrt on
/// f32 input must emit f32 WGSL, not f64, to avoid zeros on Metal).
///
/// # Arguments
///
/// * `ctx` - The lowering context with execution model and type checker
/// * `args` - AST expressions for the intrinsic arguments
/// * `declared` - The declared return type from type checking
/// * `span` - Source span for error reporting
///
/// # Returns
///
/// The adjusted return type: f32 when in a GpuKernel and *any* argument is f32,
/// else the declared type. Multi-argument intrinsics (clamp/mix/smoothstep/
/// atan2/step) may carry the f32 witness in any position, so all args are
/// scanned rather than just the first.
pub fn gpu_math_return_type(
    ctx: &LoweringContext,
    args: &[Expression],
    declared: Type,
    span: Span,
) -> Type {
    if ctx.body.execution_model == ExecutionModel::GpuKernel && !args.is_empty() {
        for arg_expr in args {
            if let Some(arg_ty) = ctx.type_checker.get_type(arg_expr.id) {
                if arg_ty.kind == TypeKind::F32 {
                    return Type::new(TypeKind::F32, span);
                }
            }
        }
    }
    declared
}
