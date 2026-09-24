// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::body::BindingResidency as MirResidency;
use crate::mir::{
    Constant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::rt;

use crate::ast::literal::Literal;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::{
    collection_slot_type, conform_operand_to_collection_slot, donate_operand_to_container,
    lower_stored_value, ELEMENT_SLOT, MAP_VALUE_SLOT,
};
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::{
    coerce_rvalue_in, ensure_place, release_coerced_source, resolve_arg_type,
    spellings_of_one_value, wrap_for_optional_slot,
};

fn assign_to_identifier(
    ctx: &mut LoweringContext,
    id_expr: &Expression,
    op: &crate::ast::operator::AssignmentOp,
    rhs: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let ExpressionKind::Identifier(name, _) = &id_expr.node {
        let rhs_watermark = ctx.body.local_decls.len();
        let val = lower_expression(ctx, rhs, None)?;

        if let Some(&local) = ctx.variable_map.get(name.as_str()) {
            match op {
                crate::ast::operator::AssignmentOp::Assign => {
                    assign_to_var_simple(ctx, local, val.clone(), expr, dest, &rhs_watermark)
                }
                crate::ast::operator::AssignmentOp::AssignAdd
                | crate::ast::operator::AssignmentOp::AssignSub
                | crate::ast::operator::AssignmentOp::AssignMul
                | crate::ast::operator::AssignmentOp::AssignDiv
                | crate::ast::operator::AssignmentOp::AssignMod => {
                    assign_to_var_compound(ctx, local, op, val.clone(), expr)?;
                    finalize_assign_result(ctx, val, dest, expr, rhs_watermark)
                }
            }
        } else {
            Err(LoweringError::undefined_variable(name, expr.span))
        }
    } else {
        Err(LoweringError::unsupported_lhs(
            "Expected identifier",
            expr.span,
        ))
    }
}

fn assign_to_var_simple(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    val: Operand,
    expr: &Expression,
    dest: Option<Place>,
    rhs_watermark: &usize,
) -> Result<Operand, LoweringError> {
    let lhs_ty = ctx.body.local_decls[local.0].ty.clone();
    let rhs_ty = val.ty(&ctx.body).clone();

    let rvalue = if rhs_ty.kind != lhs_ty.kind && !spellings_of_one_value(&rhs_ty, &lhs_ty) {
        coerce_rvalue_in(ctx, val.clone(), &rhs_ty, &lhs_ty, expr.span)
    } else {
        Rvalue::Use(val.clone())
    };

    if ctx.is_perceus_managed(&lhs_ty.kind) {
        let rhs_place = match &rvalue {
            Rvalue::Use(Operand::Copy(p)) | Rvalue::Use(Operand::Move(p)) => Some(p.clone()),
            _ => None,
        };

        if let Some(rhs_place) = rhs_place {
            handle_managed_place_assign(ctx, local, &lhs_ty, rhs_place, expr, dest, rhs_watermark)
        } else {
            let assigned = handle_managed_nonplace_assign(ctx, local, rvalue, expr, dest)?;
            release_coerced_source(ctx, &val, &rhs_ty, &lhs_ty, *rhs_watermark, expr.span);
            Ok(assigned)
        }
    } else {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(Place::new(local), rvalue),
            span: expr.span,
        });
        finalize_assign_result(ctx, val, dest, expr, *rhs_watermark)
    }
}

fn handle_managed_place_assign(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    lhs_ty: &Type,
    rhs_place: Place,
    expr: &Expression,
    dest: Option<Place>,
    rhs_watermark: &usize,
) -> Result<Operand, LoweringError> {
    if matches!(lhs_ty.kind, TypeKind::Function(_)) {
        sync_closure_captures(ctx, local, &rhs_place);
    }

    // A whole array assigned into a gpu-resident binding is uploaded into the
    // binding's device buffer: a host value, and equally another gpu binding's
    // value (`b = a`), which the readback pass reads back before the upload
    // copies its host array. Only a binding sharing the target's device buffer
    // needs no upload.
    // TODO: a host value assigned into a gpu *scalar* is not uploaded. Once the
    // scalar has a device buffer (it holds a reduction's result), `s = 3` writes
    // only the host copy, and the next readback of `s` overwrites it with the
    // stale device value.
    let lhs_decl = &ctx.body.local_decls[local.0];
    let rhs_decl = &ctx.body.local_decls[rhs_place.local.0];
    let shares_the_device_buffer =
        rhs_decl.residency == MirResidency::Gpu && rhs_decl.device_handle == lhs_decl.device_handle;
    let should_upload = lhs_decl.residency == MirResidency::Gpu
        && !shares_the_device_buffer
        && rhs_place.projection.is_empty()
        && ctx.is_perceus_managed(&lhs_ty.kind);

    if should_upload {
        // Emit upload BEFORE the assignment so the host bytes are transferred to device first.
        emit_gpu_upload(ctx, local, &rhs_place, expr)?;
    }

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Reassign(
            Place::new(local),
            Rvalue::Use(Operand::Copy(rhs_place.clone())),
        ),
        span: expr.span,
    });
    ctx.emit_temp_drop(rhs_place.local, *rhs_watermark, expr.span);

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(local))),
            ),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(Operand::Copy(Place::new(local)))
    }
}

/// Moves the capture types recorded for `rhs_place` onto `local`, so the closure
/// `local` now holds is described by its own captures.
///
/// The closure `local` held before is released by the reassignment itself, and
/// its destructor releases that closure's captures, so none are released here.
fn sync_closure_captures(ctx: &mut LoweringContext, local: crate::mir::Local, rhs_place: &Place) {
    match ctx.body.closure_capture_types.remove(&rhs_place.local) {
        Some(new_caps) => {
            ctx.body.closure_capture_types.insert(local, new_caps);
        }
        None => {
            ctx.body.closure_capture_types.remove(&local);
        }
    }
}

fn handle_managed_nonplace_assign(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    rvalue: Rvalue,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Reassign(Place::new(local), rvalue),
        span: expr.span,
    });

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(local))),
            ),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(Operand::Copy(Place::new(local)))
    }
}

fn assign_to_var_compound(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let lhs_op = Operand::Copy(Place::new(local));
    let result_ty = ctx.body.local_decls[local.0].ty.clone();
    let watermark = ctx.body.local_decls.len();
    let combined =
        combine_compound_operands(ctx, &result_ty, op, lhs_op, val.clone(), expr, watermark)?;

    // `x op= y` stores what `x op y` yields, so the store is the plain one and
    // takes its reference counting: the target releases what it held and takes
    // the combined value, which is a fresh allocation when the operator was a
    // method call.
    if ctx.is_perceus_managed(&result_ty.kind) {
        assign_to_var_simple(ctx, local, combined, expr, None, &watermark)?;
        return Ok(());
    }

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(combined)),
        span: expr.span,
    });

    Ok(())
}

fn finalize_assign_result(
    ctx: &mut LoweringContext,
    val: Operand,
    dest: Option<Place>,
    expr: &Expression,
    rhs_watermark: usize,
) -> Result<Operand, LoweringError> {
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
            span: expr.span,
        });
        if let Operand::Copy(place) | Operand::Move(place) = &val {
            ctx.emit_temp_drop(place.local, rhs_watermark, expr.span);
        }
        Ok(Operand::Copy(d))
    } else {
        Ok(val)
    }
}

fn assign_to_member(
    ctx: &mut LoweringContext,
    member_expr: &Expression,
    op: &crate::ast::operator::AssignmentOp,
    rhs: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let ExpressionKind::Member(obj, prop) = &member_expr.node {
        // A store through a field projection hands its reference to the object
        // being written, which releases it when that object dies — so the store
        // takes a reference of its own. Reading the right-hand side as a `Copy`
        // is what funds it: moving would leave the field and the source sharing
        // one reference that both release, and the field holding freed memory
        // as soon as the first release ran.
        let rhs_watermark = ctx.body.local_decls.len();
        let lowered = lower_expression(ctx, rhs, None)?;
        let rhs_ty = resolve_arg_type(ctx, rhs, &lowered);
        let val = crate::mir::lowering::dispatch::move_to_copy(lowered);
        let obj_operand = super::value_copy::lower_projection_base(ctx, obj)?;
        let obj_ty = ctx
            .recorded_type(obj.id)
            .ok_or_else(|| LoweringError::type_not_found(obj.id, obj.span))?;

        let TypeKind::Custom(type_name, _) = &obj_ty.kind else {
            return Err(LoweringError::unsupported_lhs(
                format!("Cannot assign to member of non-struct type: {:?}", obj_ty),
                expr.span,
            ));
        };
        let field_index =
            resolve_member_field_index(type_name, prop, ctx.type_checker.type_definitions());
        let Some(idx) = field_index else {
            return Err(LoweringError::unsupported_lhs(
                format!("Cannot assign to member of non-struct type: {:?}", obj_ty),
                expr.span,
            ));
        };
        let assigned = AssignedValue {
            operand: val,
            ty: rhs_ty,
            watermark: rhs_watermark,
        };
        let slot_ty = field_slot_type(ctx, &obj_ty, type_name, idx);
        store_into_field(
            ctx,
            assigned,
            FieldTarget {
                base: obj_operand,
                base_span: obj.span,
                idx,
                slot_ty,
            },
            op,
            rhs.span,
            expr,
            dest,
        )
    } else {
        Err(LoweringError::unsupported_lhs(
            "Expected Member expression",
            expr.span,
        ))
    }
}

/// The field an assignment writes: the object holding it, its index in that
/// object's layout, and the type the field has in that object.
struct FieldTarget {
    base: Operand,
    base_span: crate::error::syntax::Span,
    idx: usize,
    slot_ty: Option<Type>,
}

/// Store the right-hand value into the field, wrapping it first when the field
/// declares an optional and the value arrived bare.
fn store_into_field(
    ctx: &mut LoweringContext,
    assigned: AssignedValue,
    target: FieldTarget,
    op: &crate::ast::operator::AssignmentOp,
    rhs_span: crate::error::syntax::Span,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let val = wrap_value_for_field_slot(ctx, assigned, target.slot_ty.as_ref(), op, rhs_span);
    let mut target_place = ensure_place(ctx, target.base, target.base_span);
    target_place.projection.push(PlaceElem::Field(target.idx));

    dispatch_member_assign(
        ctx,
        &target_place,
        op,
        val.clone(),
        target.slot_ty.as_ref(),
        expr,
    )?;
    finalize_member_result(ctx, val, dest, expr)
}

fn dispatch_member_assign(
    ctx: &mut LoweringContext,
    target_place: &Place,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    slot_ty: Option<&Type>,
    expr: &Expression,
) -> Result<(), LoweringError> {
    match op {
        crate::ast::operator::AssignmentOp::Assign => {
            assign_to_member_simple(ctx, target_place, slot_ty, val, expr)?;
        }
        crate::ast::operator::AssignmentOp::AssignAdd
        | crate::ast::operator::AssignmentOp::AssignSub
        | crate::ast::operator::AssignmentOp::AssignMul
        | crate::ast::operator::AssignmentOp::AssignDiv
        | crate::ast::operator::AssignmentOp::AssignMod => {
            assign_to_member_compound(ctx, target_place, op, val, slot_ty, expr)?;
        }
    }
    Ok(())
}

fn finalize_member_result(
    ctx: &mut LoweringContext,
    val: Operand,
    dest: Option<Place>,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(val)
    }
}

fn resolve_member_field_index(
    type_name: &str,
    prop: &Expression,
    type_defs: &std::collections::HashMap<String, crate::type_checker::context::TypeDefinition>,
) -> Option<usize> {
    match type_defs.get(type_name) {
        Some(crate::type_checker::context::TypeDefinition::Struct(def)) => {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                def.fields.iter().position(|(f, _, _)| f == field_name)
            } else {
                None
            }
        }
        Some(crate::type_checker::context::TypeDefinition::Class(def)) => {
            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                let all_fields =
                    crate::type_checker::context::collect_class_fields_all(def, type_defs);
                all_fields
                    .iter()
                    .position(|(n, _)| *n == field_name.as_str())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The right-hand side of a member assignment, carried together with what the
/// type checker recorded for it and the local watermark taken before it was
/// lowered — which is what tells a temp made for it from a pre-existing local.
struct AssignedValue {
    operand: Operand,
    ty: Type,
    watermark: usize,
}

/// Wrap a bare value assigned into a field whose declaration is an optional.
///
/// The type checker lets a `T` stand where a `T?` is declared, and a field
/// declaration is such a slot. The store writes exactly the operand it is
/// handed, so a value left bare would put the raw payload in the field and the
/// next read would take that payload for the address of an optional. A compound
/// assignment is arithmetic on the field's own type and never stores the
/// right-hand side on its own, so it passes through untouched.
fn wrap_value_for_field_slot(
    ctx: &mut LoweringContext,
    assigned: AssignedValue,
    slot_ty: Option<&Type>,
    op: &crate::ast::operator::AssignmentOp,
    span: crate::error::syntax::Span,
) -> Operand {
    if !matches!(op, crate::ast::operator::AssignmentOp::Assign) {
        return assigned.operand;
    }
    let Some(slot_ty) = slot_ty else {
        return assigned.operand;
    };
    wrap_for_optional_slot(
        ctx,
        assigned.operand,
        assigned.ty,
        slot_ty,
        assigned.watermark,
        span,
    )
    .0
}

/// The type of the field an assignment writes, as the instance being written
/// through holds it.
///
/// `Body::field_types` records a field as its class declares it, so a field
/// declared `value T` reads as the bare parameter — neither managed nor an
/// optional slot — everywhere but the class's own method bodies, where the
/// table is substituted in place for the instantiation being lowered. The
/// instance's own type arguments are what give the field the type its value
/// actually has, and that type is what decides whether the store releases what
/// it replaces and whether a bare value is wrapped for an optional slot.
fn field_slot_type(
    ctx: &LoweringContext,
    instance: &Type,
    type_name: &str,
    idx: usize,
) -> Option<Type> {
    let declared = ctx.body.field_types.get(type_name)?.get(idx)?;
    Some(crate::mir::lowering::field_type_in_instance(
        &ctx.body.class_type_params,
        type_name,
        Some(instance),
        declared,
    ))
}

fn assign_to_member_simple(
    ctx: &mut LoweringContext,
    target_place: &Place,
    slot_ty: Option<&Type>,
    val: Operand,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let field_is_managed = slot_ty.is_some_and(|ty| ctx.is_perceus_managed(&ty.kind));

    if field_is_managed {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Reassign(target_place.clone(), Rvalue::Use(val.clone())),
            span: expr.span,
        });
    } else {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(target_place.clone(), Rvalue::Use(val.clone())),
            span: expr.span,
        });
    }

    Ok(())
}

/// The type of the temp holding a compound assignment's arithmetic result.
///
/// The field's own slot states it. The property expression that named the field
/// does not: the type checker records no type against that identifier, so
/// reading it yields the error type and the temp is laid out at the
/// pointer-width integer fallback — which truncates a `float` field's sum on its
/// way back into the field.
fn compound_field_result_type(slot_ty: Option<&Type>, span: crate::error::syntax::Span) -> Type {
    slot_ty
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Error, span))
}

fn assign_to_member_compound(
    ctx: &mut LoweringContext,
    target_place: &Place,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    slot_ty: Option<&Type>,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let lhs_op = Operand::Copy(target_place.clone());
    let result_ty = compound_field_result_type(slot_ty, expr.span);
    let watermark = ctx.body.local_decls.len();
    let combined =
        combine_compound_operands(ctx, &result_ty, op, lhs_op, val.clone(), expr, watermark)?;

    // The store into the field is the plain one, for the reason the variable
    // target gives: a managed field releases what it held and takes the
    // combined value rather than copying it again.
    if ctx.is_perceus_managed(&result_ty.kind) {
        assign_to_member_simple(ctx, target_place, slot_ty, combined.clone(), expr)?;
    } else {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(target_place.clone(), Rvalue::Use(combined.clone())),
            span: expr.span,
        });
    }

    // The store took its own reference, so the temp the combination landed in
    // has one left to give back — the same balance the element target keeps.
    if let Operand::Copy(place) | Operand::Move(place) = &combined {
        ctx.emit_temp_drop(place.local, watermark, expr.span);
    }

    Ok(())
}

fn inc_ref_if_managed(ctx: &mut LoweringContext, op: &Operand, ty: &Type, expr: &Expression) {
    if ctx.is_perceus_managed(&ty.kind) {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::IncRef(place.clone()),
                span: expr.span,
            });
        }
    }
}

fn emit_map_set_call(
    ctx: &mut LoweringContext,
    obj_op: Operand,
    key_op: Operand,
    val_op: Operand,
    expr: &Expression,
) -> crate::mir::Local {
    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(rt::MAP_SET.to_string()),
    }));

    let target_bb = ctx.new_basic_block();
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, expr.span), expr.span);

    ctx.set_terminator(crate::mir::Terminator::new(
        crate::mir::TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, key_op, val_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(dummy_dest),
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);
    dummy_dest
}

/// Lower the receiver of an indexed assignment, taking an exclusive copy first
/// when the collection shares its buffer.
///
/// Writing through an index mutates in place, so without this a second binding
/// to the same collection sees the write. The equivalent `set` method already
/// goes through the same check; `Array` is fixed-size and has no CoW intrinsic,
/// so it is left alone.
fn lower_index_assign_receiver(
    ctx: &mut LoweringContext,
    obj: &Expression,
    span: crate::error::syntax::Span,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let Some(obj_ty) = ctx.type_checker.get_type(obj.id).cloned() else {
        return Ok(obj_op);
    };
    let Some(cow_name) = obj_ty
        .kind
        .as_builtin_collection()
        .and_then(crate::runtime_fns::cow_fn)
    else {
        return Ok(obj_op);
    };
    Ok(crate::mir::lowering::method_dispatch::emit_cow_check(
        ctx, obj_op, &obj_ty, cow_name, span,
    ))
}

fn assign_to_index_map(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    idx: &Expression,
    (val, val_ty): (Operand, Type),
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_index_assign_receiver(ctx, obj, expr.span)?;
    let key_watermark = ctx.body.local_decls.len();
    let (key_op, key_ty) = lower_stored_value(ctx, idx, obj_ty, ELEMENT_SLOT)?;
    let (key_op, key_src) = donate_operand_to_container(ctx, key_op, key_ty, idx.span);

    // The value arrives lowered by the assignment that owns it, so it is the one
    // stored operand that never passed through `lower_stored_value`; it is read
    // at the map's own value width here instead.
    let (val, val_ty) =
        conform_operand_to_collection_slot(ctx, val, val_ty, obj_ty, MAP_VALUE_SLOT, expr.span);

    inc_ref_if_managed(ctx, &val, &val_ty, expr);

    let _dummy_dest = emit_map_set_call(ctx, obj_op, key_op, val.clone(), expr);
    if let Some(src) = key_src {
        ctx.emit_temp_drop(src, key_watermark, idx.span);
    }

    let ret_val = match val {
        Operand::Move(p) => Operand::Copy(p),
        other => other,
    };

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(ret_val.clone())),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(ret_val)
    }
}

/// Lower `m[k] <op>= v`: read the entry, combine it, store the result.
///
/// A compound write is a read-modify-write, and its read half is the same
/// checked read `m[k]` performs — so a key the map does not hold reports that it
/// was not found rather than being quietly created. Storing the right-hand side
/// on its own, which is what dropping the operator amounted to, lost the old
/// value with no diagnostic; creating the entry instead would be the same class
/// of surprise one step further on.
///
/// The key is lowered once and used twice: read by the lookup, which only
/// borrows it, and donated to the store, which keeps it.
#[allow(clippy::too_many_arguments)]
fn compound_assign_to_index_map(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    idx: &Expression,
    op: &crate::ast::operator::AssignmentOp,
    rhs: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // The combined value is typed by the map's own value slot, not by whatever
    // the right-hand side happens to be, so combining an `int` into a float
    // value keeps the float.
    let value_ty = collection_slot_type(ctx, obj_ty, MAP_VALUE_SLOT)
        .unwrap_or_else(|| Type::new(TypeKind::Int, expr.span));

    let obj_op = lower_index_assign_receiver(ctx, obj, expr.span)?;
    let key_watermark = ctx.body.local_decls.len();
    let (key_op, key_ty) = lower_stored_value(ctx, idx, obj_ty, ELEMENT_SLOT)?;
    let key_place = ensure_place(ctx, key_op, idx.span);

    let old = emit_map_get_checked_call(
        ctx,
        obj_op.clone(),
        Operand::Copy(key_place.clone()),
        &value_ty,
        expr,
    );

    let value_watermark = ctx.body.local_decls.len();
    let rhs_op = lower_expression(ctx, rhs, None)?;
    let combined =
        combine_compound_operands(ctx, &value_ty, op, old, rhs_op, expr, value_watermark)?;

    // Combining produces a value of its own, already owning the one reference
    // the map is about to take. Donating hands that one over and releases the
    // temp it came from, where retaining it as well would leave a count the
    // temp has no scope to give back.
    let (donated_key, key_src) =
        donate_operand_to_container(ctx, Operand::Copy(key_place), key_ty, idx.span);
    // The store keeps the value, so it is handed a reference of its own, the
    // way the plain `m[k] = v` hands it one. The temp the combination landed in
    // keeps the reference it already held and gives that one back at the end of
    // its scope.
    inc_ref_if_managed(ctx, &combined, &value_ty, expr);
    let donated_val = combined;
    emit_map_set_call(ctx, obj_op, donated_key, donated_val.clone(), expr);
    if let Some(src) = key_src {
        ctx.emit_temp_drop(src, key_watermark, idx.span);
    }

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(donated_val)),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(donated_val)
    }
}

/// The binary operator a compound assignment names.
fn compound_binary_op(
    op: &crate::ast::operator::AssignmentOp,
) -> Result<crate::ast::operator::BinaryOp, LoweringError> {
    op.binary_op().ok_or_else(|| {
        LoweringError::unsupported_lhs(
            "a plain assignment combines with nothing",
            crate::error::syntax::Span::new(0, 0),
        )
    })
}

/// The rvalue a compound assignment stores back into its target: `x op y` for
/// the `x` and `y` it combines.
///
/// A type that declares the operator as a trait method — a string's `concat`,
/// a user class's own — is combined by *calling* it, exactly as the written-out
/// operator is; only a type declaring none combines with a machine
/// instruction. Reading the two spellings differently is what made `s += t`
/// add two addresses while `s + t` concatenated, and it is why all four
/// compound targets (a variable, a field, an element, a map value) come
/// through here rather than building the operation themselves.
///
/// `arg_watermark` is where the caller's own temps begin: a method call
/// releases the operand temps created at or after it, and leaves the target's
/// own binding — which is older — alone. A caller that lowered the right-hand
/// side itself passes the mark it took beforehand, so that temp is released
/// here rather than outliving the statement.
fn combine_compound_operands(
    ctx: &mut LoweringContext,
    slot_ty: &Type,
    op: &crate::ast::operator::AssignmentOp,
    lhs_op: Operand,
    rhs_op: Operand,
    expr: &Expression,
    arg_watermark: usize,
) -> Result<Operand, LoweringError> {
    let binary_op = compound_binary_op(op)?;
    let operands = crate::mir::lowering::expression::binary_expr::OperatorOperands {
        lhs_op: lhs_op.clone(),
        rhs_op: rhs_op.clone(),
    };
    // A method call already leaves its result in a temp of the method's own
    // return type. Copying that into a second temp would take a reference the
    // first one never gives back, so the call's own result is the value.
    if let Some(result) =
        crate::mir::lowering::expression::binary_expr::try_lower_operator_trait_call(
            ctx,
            slot_ty,
            &binary_op,
            operands,
            expr,
            None,
            arg_watermark,
        )?
    {
        return Ok(result);
    }
    // A machine instruction has no result slot of its own. The temp it lands in
    // is typed by the target, because the operation's recorded type is the
    // pointer-width fallback and would truncate a float sum on its way back.
    let bin_op = crate::mir::lowering::expression::binary_expr::op_to_binop(&binary_op, expr.span)?;
    let temp = ctx.push_temp(slot_ty.clone(), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(rhs_op)),
        ),
        span: expr.span,
    });
    Ok(Operand::Copy(Place::new(temp)))
}

/// Read `m[k]` the way an index read does, aborting when the key is absent.
fn emit_map_get_checked_call(
    ctx: &mut LoweringContext,
    obj_op: Operand,
    key_op: Operand,
    value_ty: &Type,
    expr: &Expression,
) -> Operand {
    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(rt::MAP_GET_CHECKED.to_string()),
    }));

    // The lookup hands back what the map still owns without raising its count,
    // so the temp holding it is a borrow: releasing it would take a reference
    // away from the entry the map is still holding.
    let temp = ctx.push_temp(value_ty.clone(), expr.span);
    if ctx.is_perceus_managed(&value_ty.kind)
        && crate::runtime_fns::hands_back_a_borrow(rt::MAP_GET_CHECKED, &value_ty.kind)
    {
        ctx.mark_borrowed_temp(temp);
    }

    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(crate::mir::Terminator::new(
        crate::mir::TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, key_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(temp),
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);
    Operand::Copy(Place::new(temp))
}

#[allow(clippy::too_many_arguments)]
fn assign_to_index_array(
    ctx: &mut LoweringContext,
    obj: &Expression,
    idx: &Expression,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    elem_ty: Option<&Type>,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // A receiver that is itself an indexed read (`self.rows[r][c] = x`) lowers
    // to a retained copy of the inner collection, held in a temp that exists
    // only for this write. The mark taken here is what tells that temp from a
    // receiver the program already had — a binding, or a field of one — which
    // outlives the write and must keep its reference.
    let receiver_watermark = ctx.body.local_decls.len();
    let obj_operand = lower_index_assign_receiver(ctx, obj, expr.span)?;
    let obj_place = ensure_place(ctx, obj_operand, obj.span);
    let receiver_local = obj_place.local;

    let index_operand = lower_expression(ctx, idx, None)?;
    let index_local = normalize_index(ctx, index_operand, idx)?;

    let mut target_place = obj_place;
    target_place.projection.push(PlaceElem::Index(index_local));

    let val = match val {
        Operand::Move(p) => Operand::Copy(p),
        other => other,
    };

    match op {
        crate::ast::operator::AssignmentOp::Assign => {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(target_place, Rvalue::Use(val.clone())),
                span: expr.span,
            });
        }
        crate::ast::operator::AssignmentOp::AssignAdd
        | crate::ast::operator::AssignmentOp::AssignSub
        | crate::ast::operator::AssignmentOp::AssignMul
        | crate::ast::operator::AssignmentOp::AssignDiv
        | crate::ast::operator::AssignmentOp::AssignMod => {
            assign_to_index_compound(ctx, &target_place, op, val.clone(), elem_ty, expr)?;
        }
    }

    // The write is done, so the copy taken to reach through has nothing left to
    // do. A receiver older than the mark is the program's own and is left alone.
    ctx.emit_temp_drop(receiver_local, receiver_watermark, expr.span);

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(val)
    }
}

fn normalize_index(
    ctx: &mut LoweringContext,
    index_operand: Operand,
    idx: &Expression,
) -> Result<crate::mir::Local, LoweringError> {
    match index_operand {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => Ok(p.local),
        _ => {
            let ty = ctx
                .type_checker
                .get_type(idx.id)
                .cloned()
                .unwrap_or_else(|| Type::new(TypeKind::Int, idx.span));
            let temp = ctx.push_temp(ty, idx.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(index_operand)),
                span: idx.span,
            });
            Ok(temp)
        }
    }
}

fn assign_to_index_compound(
    ctx: &mut LoweringContext,
    target_place: &Place,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    elem_ty: Option<&Type>,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let lhs_op = Operand::Copy(target_place.clone());
    // The result is a value of the element's own type. Typing it `int`
    // regardless truncated a float element's sum on its way into the temp, and
    // the truncated value was what got stored back.
    let result_ty = compound_field_result_type(elem_ty, expr.span);
    let watermark = ctx.body.local_decls.len();
    let combined =
        combine_compound_operands(ctx, &result_ty, op, lhs_op, val.clone(), expr, watermark)?;

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target_place.clone(), Rvalue::Use(combined.clone())),
        span: expr.span,
    });

    // The store took its own reference to the combined value, so the temp that
    // held it has one left to give back. A machine result is not managed and
    // the release is skipped for it.
    if let Operand::Copy(place) | Operand::Move(place) = &combined {
        ctx.emit_temp_drop(place.local, watermark, expr.span);
    }

    Ok(())
}

fn emit_gpu_upload(
    ctx: &mut LoweringContext,
    gpu_local: crate::mir::Local,
    host_place: &Place,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let Some(handle) = ctx.body.local_decls[gpu_local.0].device_handle else {
        return Err(LoweringError::unsupported_expression(
            "GPU-resident binding has no device handle",
            expr.span,
        ));
    };

    let handle_operand = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Int, expr.span),
        literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(handle.0 as i64)),
    }));

    let array_operand = Operand::Copy(host_place.clone());

    let func_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: Literal::Identifier(crate::mir::residency::UPLOAD_FN.to_string()),
    }));

    let target_bb = ctx.new_basic_block();
    let _result_temp = ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![handle_operand, array_operand],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(_result_temp),
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);
    Ok(())
}

pub(crate) fn lower_assignment_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Assignment(lhs, op, rhs) = &expr.node else {
        unreachable!()
    };
    match &**lhs {
        crate::ast::expression::LeftHandSideExpression::Identifier(id_expr) => {
            assign_to_identifier(ctx, id_expr, op, rhs, expr, dest)
        }
        crate::ast::expression::LeftHandSideExpression::Member(member_expr) => {
            assign_to_member(ctx, member_expr, op, rhs, expr, dest)
        }
        crate::ast::expression::LeftHandSideExpression::Index(index_expr) => {
            if let ExpressionKind::Index(obj, idx) = &index_expr.node {
                let obj_ty = ctx.type_checker.get_type(obj.id).cloned();
                if let Some(obj_ty) = &obj_ty {
                    if obj_ty.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map) {
                        if !matches!(op, crate::ast::operator::AssignmentOp::Assign) {
                            return compound_assign_to_index_map(
                                ctx, obj, obj_ty, idx, op, rhs, expr, dest,
                            );
                        }
                        let val = lower_stored_value(ctx, rhs, obj_ty, MAP_VALUE_SLOT)?;
                        return assign_to_index_map(ctx, obj, obj_ty, idx, val, expr, dest);
                    }
                }
                let is_plain_write = matches!(op, crate::ast::operator::AssignmentOp::Assign);
                let val = match obj_ty.as_ref().filter(|_| is_plain_write) {
                    Some(obj_ty) => lower_stored_value(ctx, rhs, obj_ty, ELEMENT_SLOT)?.0,
                    None => lower_expression(ctx, rhs, None)?,
                };
                // A compound write combines into a value of the element's own
                // type, so the element type travels with the call the way a
                // field's declaring slot does.
                let elem_ty = obj_ty
                    .as_ref()
                    .and_then(|ty| collection_slot_type(ctx, ty, ELEMENT_SLOT));
                assign_to_index_array(ctx, obj, idx, op, val, elem_ty.as_ref(), expr, dest)
            } else {
                Err(LoweringError::unsupported_lhs(
                    "Expected Index expression",
                    expr.span,
                ))
            }
        }
    }
}
