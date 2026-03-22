// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method-interception registry for MIR lowering.
//!
//! Built-in collection methods (e.g. `length`, `push`) are lowered directly to
//! MIR constructs rather than through the generic class-method dispatch path.
//! This module centralises that interception logic so that:
//!
//! - Each method's type guard lives alongside its handler.
//! - Adding a new intercepted method means adding one entry to [`REGISTRY`].
//! - The dispatch site in `control_flow.rs` stays a simple registry lookup.
//!
//! # Adding a new intercepted method
//!
//! 1. Write a `matches_*` predicate that returns `true` for the applicable
//!    receiver types and argument counts.
//! 2. Write a `handle_*` function that emits the MIR for the call.
//! 3. Add an [`InterceptedMethod`] entry to [`REGISTRY`].

use crate::ast::expression::Expression;
use crate::ast::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind};

use super::{lower_expression, LoweringContext};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A predicate deciding whether this entry applies to the given receiver type
/// and argument count.
pub type MatchFn = fn(obj_ty_kind: &TypeKind, arg_count: usize) -> bool;

/// The lowering handler for an intercepted method.  Receives all call-site
/// context and emits the appropriate MIR, returning the result operand.
pub type InterceptFn = fn(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError>;

/// A single entry in the interception registry.
pub struct InterceptedMethod {
    pub method_name: &'static str,
    pub matches: MatchFn,
    pub handler: InterceptFn,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// All intercepted built-in collection methods, in priority order.
///
/// The first entry whose `method_name` and `matches` predicate agree with the
/// call site is used; later entries are not checked.
pub const REGISTRY: &[InterceptedMethod] = &[
    InterceptedMethod {
        method_name: "length",
        matches: matches_length,
        handler: handle_length,
    },
    InterceptedMethod {
        method_name: "element_at",
        matches: matches_element_at,
        handler: handle_element_at,
    },
    // `get` is an alias for `element_at` on List/Array/Tuple.
    InterceptedMethod {
        method_name: "get",
        matches: matches_element_at,
        handler: handle_element_at,
    },
    InterceptedMethod {
        method_name: "push",
        matches: matches_push,
        handler: handle_push,
    },
    InterceptedMethod {
        method_name: "set",
        matches: matches_set,
        handler: handle_set,
    },
    InterceptedMethod {
        method_name: "insert",
        matches: matches_insert,
        handler: handle_insert,
    },
];

/// Look up a handler for the given method name, receiver type, and argument count.
///
/// Returns `Some(handler)` if a registered entry matches, or `None` if no
/// interception applies — allowing the caller to fall through to the generic
/// class-method dispatch path.
pub fn lookup(method_name: &str, obj_ty_kind: &TypeKind, arg_count: usize) -> Option<InterceptFn> {
    REGISTRY
        .iter()
        .find(|m| m.method_name == method_name && (m.matches)(obj_ty_kind, arg_count))
        .map(|m| m.handler)
}

// ---------------------------------------------------------------------------
// Match predicates
// ---------------------------------------------------------------------------

fn matches_length(ty: &TypeKind, _arg_count: usize) -> bool {
    matches!(
        ty,
        TypeKind::Tuple(_)
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::String
    ) || matches!(
        ty,
        TypeKind::Custom(name, _)
            if BuiltinCollectionKind::from_name(name).is_some() || name == "Tuple"
    )
}

fn matches_element_at(ty: &TypeKind, arg_count: usize) -> bool {
    arg_count == 1
        && (matches!(
            ty,
            TypeKind::Tuple(_) | TypeKind::List(_) | TypeKind::Array(_, _)
        ) || matches!(
            ty,
            TypeKind::Custom(name, _)
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) || name == "Tuple"
        ))
}

fn matches_push(ty: &TypeKind, arg_count: usize) -> bool {
    arg_count == 1
        && (matches!(ty, TypeKind::List(_))
            || matches!(
                ty,
                TypeKind::Custom(name, _)
                    if BuiltinCollectionKind::from_name(name)
                        == Some(BuiltinCollectionKind::List)
            ))
}

fn matches_set(ty: &TypeKind, arg_count: usize) -> bool {
    arg_count == 2
        && (matches!(ty, TypeKind::List(_) | TypeKind::Array(_, _))
            || matches!(
                ty,
                TypeKind::Custom(name, _)
                    if matches!(
                        BuiltinCollectionKind::from_name(name),
                        Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                    )
            ))
}

fn matches_insert(ty: &TypeKind, arg_count: usize) -> bool {
    arg_count == 2
        && (matches!(ty, TypeKind::List(_))
            || matches!(
                ty,
                TypeKind::Custom(name, _)
                    if BuiltinCollectionKind::from_name(name)
                        == Some(BuiltinCollectionKind::List)
            ))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Lower `.length()` on List / Array / Tuple / Map / Set / String.
///
/// Emits `Rvalue::Len` which reads the `LEN` field from the RC+LEN+DATA memory
/// layout without going through a runtime function call.
fn handle_length(
    ctx: &mut LoweringContext,
    span: &Span,
    _call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    _args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    // Drop obj_local for any Copy operand (including field-projected ones).
    // Perceus now IncRefs all Copy operands whose source place is managed
    // (including field projections via the updated is_place_managed), so
    // emit_temp_drop is safe for all copies.
    // For Move operands Perceus does not IncRef, so no drop is needed.
    let obj_op_copy_src = if let Operand::Copy(ref p) = obj_op {
        Some(p.local)
    } else {
        None
    };

    // Materialise the object into a temp local so we can form a Place for Rvalue::Len.
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
        kind: StatementKind::Assign(destination, Rvalue::Len(Place::new(obj_local))),
        span: *span,
    });

    if let Some(src_local) = obj_op_copy_src {
        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
        // Also drop the source of the Copy (e.g. an inline constructor temp).
        // Perceus IncRef'd the source for the Copy, so we need a matching
        // DecRef (via StorageDead) for the source local itself.
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
    Ok(op)
}

/// Lower `.element_at(i)` / `.get(i)` on List / Array / Tuple.
///
/// Emits a MIR index projection (`place[index]`) rather than a runtime call.
fn handle_element_at(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    // See comment in handle_length: Perceus now IncRefs all managed Copy
    // operands including field projections, so emit_temp_drop is safe.
    let obj_op_copy_src = if let Operand::Copy(ref p) = obj_op {
        Some(p.local)
    } else {
        None
    };
    let index_op = lower_expression(ctx, &args[0], None)?;

    // Materialise the object into a temp local.
    let obj_local = ctx.push_temp(obj_ty.clone(), *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
        span: *span,
    });

    // Ensure the index is in a simple local (no projection) for the Index elem.
    let index_local = match index_op {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => {
            let temp = ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(index_op)),
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
        kind: StatementKind::Assign(destination, Rvalue::Use(Operand::Copy(indexed_place))),
        span: *span,
    });

    // Only drop obj_local when a Copy was used (Perceus will have IncRef'd).
    if let Some(src_local) = obj_op_copy_src {
        ctx.emit_temp_drop(obj_local, obj_watermark, *span);
        // Also drop the source of the Copy (e.g. an inline constructor temp).
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
    Ok(op)
}

/// Lower `.push(item)` on List.
///
/// Emits a call to the `miri_rt_list_push` runtime function.
fn handle_push(
    ctx: &mut LoweringContext,
    span: &Span,
    _call_expr_id: usize,
    obj: &Expression,
    _obj_ty: &Type,
    args: &[Expression],
    _dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let item_op = lower_expression(ctx, &args[0], None)?;

    // Materialise the item into a temp local so we can take its address.
    let item_ty = item_op.ty(&ctx.body).clone();
    let item_local = ctx.push_temp(item_ty, args[0].span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_op)),
        span: args[0].span,
    });

    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier("miri_rt_list_push".to_string()),
    }));

    let target_bb = ctx.new_basic_block();
    // push returns void, but Call requires a destination — use a dummy local.
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
    Ok(Operand::Copy(Place::new(dummy_dest)))
}

/// Lower `.set(index, item)` on List / Array.
///
/// Emits a MIR indexed assignment (`obj[index] = item`) rather than a runtime
/// call, which lets the existing codegen handle element RC correctly.
fn handle_set(
    ctx: &mut LoweringContext,
    span: &Span,
    _call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    args: &[Expression],
    _dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let index_op = lower_expression(ctx, &args[0], None)?;
    let item_op = lower_expression(ctx, &args[1], None)?;

    // Materialise the object into a temp local.
    let obj_local = ctx.push_temp(obj_ty.clone(), *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(obj_local), Rvalue::Use(obj_op)),
        span: *span,
    });

    // Ensure the index is in a simple local for the Index projection.
    let index_local = match index_op {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => {
            let temp = ctx.push_temp(Type::new(TypeKind::Int, args[0].span), args[0].span);
            ctx.push_statement(crate::mir::Statement {
                kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(index_op)),
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

    Ok(Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Void, *span),
        literal: crate::ast::literal::Literal::None,
    })))
}

/// Lower `.insert(index, item)` on List.
///
/// Emits a call to the `miri_rt_list_insert` runtime function.
fn handle_insert(
    ctx: &mut LoweringContext,
    span: &Span,
    _call_expr_id: usize,
    obj: &Expression,
    _obj_ty: &Type,
    args: &[Expression],
    _dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let index_op = lower_expression(ctx, &args[0], None)?;
    let item_op = lower_expression(ctx, &args[1], None)?;

    // Materialise the item into a temp local so we can take its address.
    let item_ty = item_op.ty(&ctx.body).clone();
    let item_local = ctx.push_temp(item_ty, args[1].span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(item_local), Rvalue::Use(item_op)),
        span: args[1].span,
    });

    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier("miri_rt_list_insert".to_string()),
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
    Ok(Operand::Copy(Place::new(result_temp)))
}
