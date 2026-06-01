// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{BinOp, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind};
use crate::runtime_fns::rt;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::{coerce_rvalue, ensure_place, resolve_type};

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
        Err(LoweringError::unsupported_lhs("Expected identifier", expr.span))
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

    let rvalue = if rhs_ty.kind != lhs_ty.kind {
        coerce_rvalue(val.clone(), &rhs_ty, &lhs_ty)
    } else {
        Rvalue::Use(val.clone())
    };

    if ctx.is_perceus_managed(&lhs_ty.kind) {
        let rhs_place = match &rvalue {
            Rvalue::Use(Operand::Copy(p)) | Rvalue::Use(Operand::Move(p)) => Some(p.clone()),
            _ => None,
        };

        if let Some(rhs_place) = rhs_place {
            handle_managed_place_assign(
                ctx, local, &lhs_ty, rhs_place, expr, dest, rhs_watermark,
            )
        } else {
            handle_managed_nonplace_assign(ctx, local, rvalue, expr, dest)
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
        sync_closure_captures(ctx, local, &rhs_place, expr.span);
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

fn sync_closure_captures(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    rhs_place: &Place,
    span: crate::error::syntax::Span,
) {
    if let Some(old_caps) = ctx.body.closure_capture_types.get(&local).cloned() {
        for (cap_idx, cap_ty) in old_caps.iter().enumerate() {
            if crate::mir::types::MirType::from_type_kind(&cap_ty.kind)
                .is_managed(&ctx.body.auto_copy_types, &ctx.body.type_params)
            {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::DecRef(Place {
                        local,
                        projection: vec![PlaceElem::Field(cap_idx)],
                    }),
                    span,
                });
            }
        }
    }

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
    let bin_op = match op {
        crate::ast::operator::AssignmentOp::AssignAdd => BinOp::Add,
        crate::ast::operator::AssignmentOp::AssignSub => BinOp::Sub,
        crate::ast::operator::AssignmentOp::AssignMul => BinOp::Mul,
        crate::ast::operator::AssignmentOp::AssignDiv => BinOp::Div,
        crate::ast::operator::AssignmentOp::AssignMod => BinOp::Rem,
        _ => unreachable!(),
    };

    let lhs_op = Operand::Copy(Place::new(local));
    let result_ty = ctx.body.local_decls[local.0].ty.clone();
    let temp = ctx.push_temp(result_ty, expr.span);

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(val.clone())),
        ),
        span: expr.span,
    });

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Copy(Place::new(temp))),
        ),
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
        let val = lower_expression(ctx, rhs, None)?;
        let obj_operand = lower_expression(ctx, obj, None)?;
        let obj_ty = ctx
            .type_checker
            .get_type(obj.id)
            .ok_or_else(|| LoweringError::type_not_found(obj.id, obj.span))?;

        if let TypeKind::Custom(type_name, _) = &obj_ty.kind {
            let field_index = resolve_member_field_index(
                type_name,
                prop,
                &ctx.type_checker.global_type_definitions.clone(),
            );
            if let Some(idx) = field_index {
                let obj_place = ensure_place(ctx, obj_operand, obj.span);
                let mut target_place = obj_place;
                target_place.projection.push(PlaceElem::Field(idx));

                dispatch_member_assign(ctx, &target_place, op, val.clone(), type_name, idx, prop, expr)?;
                finalize_member_result(ctx, val, dest, expr)
            } else {
                Err(LoweringError::unsupported_lhs(
                    format!("Cannot assign to member of non-struct type: {:?}", obj_ty),
                    expr.span,
                ))
            }
        } else {
            Err(LoweringError::unsupported_lhs(
                format!("Cannot assign to member of non-struct type: {:?}", obj_ty),
                expr.span,
            ))
        }
    } else {
        Err(LoweringError::unsupported_lhs(
            "Expected Member expression",
            expr.span,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_member_assign(
    ctx: &mut LoweringContext,
    target_place: &Place,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    type_name: &str,
    idx: usize,
    prop: &Expression,
    expr: &Expression,
) -> Result<(), LoweringError> {
    match op {
        crate::ast::operator::AssignmentOp::Assign => {
            assign_to_member_simple(ctx, target_place, type_name, idx, val, expr)?;
        }
        crate::ast::operator::AssignmentOp::AssignAdd
        | crate::ast::operator::AssignmentOp::AssignSub
        | crate::ast::operator::AssignmentOp::AssignMul
        | crate::ast::operator::AssignmentOp::AssignDiv
        | crate::ast::operator::AssignmentOp::AssignMod => {
            assign_to_member_compound(ctx, target_place, op, val, prop, expr)?;
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
    type_defs: &std::collections::HashMap<
        String,
        crate::type_checker::context::TypeDefinition,
    >,
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
                all_fields.iter().position(|(n, _)| *n == field_name.as_str())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn assign_to_member_simple(
    ctx: &mut LoweringContext,
    target_place: &Place,
    type_name: &str,
    idx: usize,
    val: Operand,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let field_is_managed = if let Some(ft) = ctx
        .body
        .field_types
        .get(type_name)
        .and_then(|fs| fs.get(idx))
    {
        let kind = ft.kind.clone();
        ctx.is_perceus_managed(&kind)
    } else {
        false
    };

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

fn assign_to_member_compound(
    ctx: &mut LoweringContext,
    target_place: &Place,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    prop: &Expression,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let bin_op = match op {
        crate::ast::operator::AssignmentOp::AssignAdd => BinOp::Add,
        crate::ast::operator::AssignmentOp::AssignSub => BinOp::Sub,
        crate::ast::operator::AssignmentOp::AssignMul => BinOp::Mul,
        crate::ast::operator::AssignmentOp::AssignDiv => BinOp::Div,
        crate::ast::operator::AssignmentOp::AssignMod => BinOp::Rem,
        _ => unreachable!(),
    };

    let lhs_op = Operand::Copy(target_place.clone());
    let result_ty = resolve_type(ctx.type_checker, prop);
    let temp = ctx.push_temp(result_ty, expr.span);

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(val.clone())),
        ),
        span: expr.span,
    });

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target_place.clone(),
            Rvalue::Use(Operand::Copy(Place::new(temp))),
        ),
        span: expr.span,
    });

    Ok(())
}

fn inc_ref_if_managed(
    ctx: &mut LoweringContext,
    op: &Operand,
    ty: &Type,
    expr: &Expression,
) {
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
            destination: Place::new(dummy_dest),
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);
    dummy_dest
}

fn assign_to_index_map(
    ctx: &mut LoweringContext,
    obj: &Expression,
    idx: &Expression,
    val: Operand,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let key_op = lower_expression(ctx, idx, None)?;

    let val_ty = val.ty(&ctx.body).clone();
    inc_ref_if_managed(ctx, &val, &val_ty, expr);

    let key_ty = key_op.ty(&ctx.body).clone();
    inc_ref_if_managed(ctx, &key_op, &key_ty, expr);

    let _dummy_dest = emit_map_set_call(ctx, obj_op, key_op, val.clone(), expr);

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

fn assign_to_index_array(
    ctx: &mut LoweringContext,
    obj: &Expression,
    idx: &Expression,
    op: &crate::ast::operator::AssignmentOp,
    val: Operand,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_operand = lower_expression(ctx, obj, None)?;
    let obj_place = ensure_place(ctx, obj_operand, obj.span);

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
            assign_to_index_compound(ctx, &target_place, op, val.clone(), expr)?;
        }
    }

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
    expr: &Expression,
) -> Result<(), LoweringError> {
    let bin_op = match op {
        crate::ast::operator::AssignmentOp::AssignAdd => BinOp::Add,
        crate::ast::operator::AssignmentOp::AssignSub => BinOp::Sub,
        crate::ast::operator::AssignmentOp::AssignMul => BinOp::Mul,
        crate::ast::operator::AssignmentOp::AssignDiv => BinOp::Div,
        crate::ast::operator::AssignmentOp::AssignMod => BinOp::Rem,
        _ => unreachable!(),
    };

    let lhs_op = Operand::Copy(target_place.clone());
    // This temp is allocated but not used in compound array assign; left for compatibility
    let _temp = ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(_temp),
            Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(val.clone())),
        ),
        span: expr.span,
    });

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target_place.clone(),
            Rvalue::Use(Operand::Copy(Place::new(_temp))),
        ),
        span: expr.span,
    });

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
                if let Some(obj_ty) = ctx.type_checker.get_type(obj.id) {
                    if obj_ty.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map) {
                        let val = lower_expression(ctx, rhs, None)?;
                        return assign_to_index_map(ctx, obj, idx, val, expr, dest);
                    }
                }
                let val = lower_expression(ctx, rhs, None)?;
                assign_to_index_array(ctx, obj, idx, op, val, expr, dest)
            } else {
                Err(LoweringError::unsupported_lhs(
                    "Expected Index expression",
                    expr.span,
                ))
            }
        }
    }
}
