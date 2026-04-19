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
            if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                let rhs_watermark = ctx.body.local_decls.len();
                let val = lower_expression(ctx, rhs, None)?;

                if let Some(&local) = ctx.variable_map.get(name.as_str()) {
                    match op {
                        crate::ast::operator::AssignmentOp::Assign => {
                            let lhs_ty = ctx.body.local_decls[local.0].ty.clone();
                            let rhs_ty = val.ty(&ctx.body).clone();

                            let rvalue = if rhs_ty.kind != lhs_ty.kind {
                                coerce_rvalue(val.clone(), &rhs_ty, &lhs_ty)
                            } else {
                                Rvalue::Use(val.clone())
                            };

                            // For managed-type reassignment, emit a `Reassign` statement.
                            // The Perceus RC pass will insert the appropriate IncRef/DecRef
                            // around it:
                            //   - For a Copy-of-place rhs: IncRef(rhs) then DecRef(lhs)
                            //     (alias-safe "inc-then-dec" order).
                            //   - For a non-place rhs (Cast, Aggregate, etc.): DecRef(lhs)
                            //     only.
                            // Using Copy (not Move) for place rhs lets Perceus handle the
                            // IncRef automatically. The rhs temp is still dropped via
                            // emit_temp_drop which triggers DecRef at its StorageDead.
                            if ctx.is_perceus_managed(&lhs_ty.kind) {
                                let rhs_place = match &rvalue {
                                    Rvalue::Use(Operand::Copy(p))
                                    | Rvalue::Use(Operand::Move(p)) => Some(p.clone()),
                                    _ => None,
                                };

                                if let Some(rhs_place) = rhs_place {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Reassign(
                                            Place::new(local),
                                            Rvalue::Use(Operand::Copy(rhs_place.clone())),
                                        ),
                                        span: expr.span,
                                    });
                                    // Drop any freshly-created rhs temp (e.g. from a
                                    // constructor or function call). Named locals
                                    // (rhs_place.local < rhs_watermark) are skipped.
                                    ctx.emit_temp_drop(rhs_place.local, rhs_watermark, expr.span);

                                    // Assignment evaluates to the assigned value.
                                    if let Some(d) = dest {
                                        ctx.push_statement(crate::mir::Statement {
                                            kind: MirStatementKind::Assign(
                                                d.clone(),
                                                Rvalue::Use(Operand::Copy(Place::new(local))),
                                            ),
                                            span: expr.span,
                                        });
                                        return Ok(Operand::Copy(d));
                                    } else {
                                        // Return Copy of lhs so lower_statement's temp
                                        // properly borrows (IncRef) then releases
                                        // (DecRef at StorageDead), netting zero change.
                                        return Ok(Operand::Copy(Place::new(local)));
                                    }
                                } else {
                                    // Non-place rvalue (e.g. Cast): Perceus will emit
                                    // DecRef(lhs) for the Reassign, and no IncRef for the
                                    // non-place rhs.  This transfers ownership of the rhs
                                    // temp into `local`.
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Reassign(Place::new(local), rvalue),
                                        span: expr.span,
                                    });
                                    // Return the lhs so callers see the updated local.
                                    if let Some(d) = dest {
                                        ctx.push_statement(crate::mir::Statement {
                                            kind: MirStatementKind::Assign(
                                                d.clone(),
                                                Rvalue::Use(Operand::Copy(Place::new(local))),
                                            ),
                                            span: expr.span,
                                        });
                                        return Ok(Operand::Copy(d));
                                    } else {
                                        return Ok(Operand::Copy(Place::new(local)));
                                    }
                                }
                            } else {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(Place::new(local), rvalue),
                                    span: expr.span,
                                });
                            }
                        }
                        crate::ast::operator::AssignmentOp::AssignAdd
                        | crate::ast::operator::AssignmentOp::AssignSub
                        | crate::ast::operator::AssignmentOp::AssignMul
                        | crate::ast::operator::AssignmentOp::AssignDiv
                        | crate::ast::operator::AssignmentOp::AssignMod => {
                            // Desugar: x op= y -> x = x op y
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
                                    Rvalue::BinaryOp(
                                        bin_op,
                                        Box::new(lhs_op),
                                        Box::new(val.clone()),
                                    ),
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
                        }
                    }

                    // Assignment evaluates to the assigned value
                    if let Some(d) = dest {
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
                            span: expr.span,
                        });
                        // When the result is copied to a different dest, release the RHS
                        // temp's ownership — the dest now holds the reference.
                        if let Operand::Copy(place) | Operand::Move(place) = &val {
                            ctx.emit_temp_drop(place.local, rhs_watermark, expr.span);
                        }
                        Ok(Operand::Copy(d))
                    } else {
                        // No dest: return the RHS operand directly. The caller owns
                        // its RC reference and is responsible for releasing it.
                        Ok(val)
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
        crate::ast::expression::LeftHandSideExpression::Member(member_expr) => {
            // Member assignment: a.b = x
            // Lower the member expression to get the object and field
            if let ExpressionKind::Member(obj, prop) = &member_expr.node {
                let val = lower_expression(ctx, rhs, None)?;

                // Get the object operand
                let obj_operand = lower_expression(ctx, obj, None)?;

                // Get the object's type to find field index
                let obj_ty = ctx
                    .type_checker
                    .get_type(obj.id)
                    .ok_or_else(|| LoweringError::type_not_found(obj.id, obj.span))?;

                if let TypeKind::Custom(type_name, _) = &obj_ty.kind {
                    let field_index = match ctx.type_checker.global_type_definitions.get(type_name)
                    {
                        Some(crate::type_checker::context::TypeDefinition::Struct(def)) => {
                            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                                def.fields.iter().position(|(f, _, _)| f == field_name)
                            } else {
                                None
                            }
                        }
                        Some(crate::type_checker::context::TypeDefinition::Class(def)) => {
                            if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                                // Compute global field index across the full inheritance chain.
                                let all_fields =
                                    crate::type_checker::context::collect_class_fields_all(
                                        def,
                                        &ctx.type_checker.global_type_definitions,
                                    );
                                all_fields
                                    .iter()
                                    .position(|(n, _)| *n == field_name.as_str())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(idx) = field_index {
                        let obj_place = ensure_place(ctx, obj_operand, obj.span);

                        // Create field projection
                        let mut target_place = obj_place;
                        target_place.projection.push(PlaceElem::Field(idx));

                        // Handle simple assignment vs compound assignment
                        match op {
                            crate::ast::operator::AssignmentOp::Assign => {
                                // When the field type is managed, emit Reassign so that the
                                // Perceus pass inserts DecRef(old_field) before the store.
                                let field_is_managed = if let Some(ft) = ctx
                                    .body
                                    .field_types
                                    .get(type_name.as_str())
                                    .and_then(|fs| fs.get(idx))
                                {
                                    let kind = ft.kind.clone();
                                    ctx.is_perceus_managed(&kind)
                                } else {
                                    false
                                };
                                if field_is_managed {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Reassign(
                                            target_place,
                                            Rvalue::Use(val.clone()),
                                        ),
                                        span: expr.span,
                                    });
                                } else {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            target_place,
                                            Rvalue::Use(val.clone()),
                                        ),
                                        span: expr.span,
                                    });
                                }
                            }
                            crate::ast::operator::AssignmentOp::AssignAdd
                            | crate::ast::operator::AssignmentOp::AssignSub
                            | crate::ast::operator::AssignmentOp::AssignMul
                            | crate::ast::operator::AssignmentOp::AssignDiv
                            | crate::ast::operator::AssignmentOp::AssignMod => {
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
                                        Rvalue::BinaryOp(
                                            bin_op,
                                            Box::new(lhs_op),
                                            Box::new(val.clone()),
                                        ),
                                    ),
                                    span: expr.span,
                                });

                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        target_place,
                                        Rvalue::Use(Operand::Copy(Place::new(temp))),
                                    ),
                                    span: expr.span,
                                });
                            }
                        }
                        if let Some(d) = dest {
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(d));
                        } else {
                            return Ok(val);
                        }
                    }
                }
                Err(LoweringError::unsupported_lhs(
                    format!("Cannot assign to member of non-struct type: {:?}", obj_ty),
                    expr.span,
                ))
            } else {
                Err(LoweringError::unsupported_lhs(
                    "Expected Member expression",
                    expr.span,
                ))
            }
        }
        #[allow(clippy::needless_return)]
        crate::ast::expression::LeftHandSideExpression::Index(index_expr) => {
            // Index assignment: a[i] = x
            if let ExpressionKind::Index(obj, idx) = &index_expr.node {
                // Intercept map index write: m[key] = value → miri_rt_map_set(m, key, value)
                if let Some(obj_ty) = ctx.type_checker.get_type(obj.id) {
                    if obj_ty.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map) {
                        let val = lower_expression(ctx, rhs, None)?;
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let key_op = lower_expression(ctx, idx, None)?;

                        // Donate managed val/key references to the map by emitting
                        // explicit IncRef statements. The original locals' StorageDead
                        // will emit the matching DecRef, leaving RC=1 in the map.
                        let val_ty = val.ty(&ctx.body).clone();
                        if ctx.is_perceus_managed(&val_ty.kind) {
                            if let Operand::Copy(place) | Operand::Move(place) = &val {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::IncRef(place.clone()),
                                    span: expr.span,
                                });
                            }
                        }
                        let val_arg = val.clone();

                        let key_ty = key_op.ty(&ctx.body).clone();
                        if ctx.is_perceus_managed(&key_ty.kind) {
                            if let Operand::Copy(place) | Operand::Move(place) = &key_op {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::IncRef(place.clone()),
                                    span: expr.span,
                                });
                            }
                        }
                        let key_arg = key_op;

                        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                            span: expr.span,
                            ty: Type::new(TypeKind::Identifier, expr.span),
                            literal: crate::ast::literal::Literal::Identifier(
                                rt::MAP_SET.to_string(),
                            ),
                        }));

                        let target_bb = ctx.new_basic_block();
                        let dummy_dest =
                            ctx.push_temp(Type::new(TypeKind::Void, expr.span), expr.span);

                        ctx.set_terminator(crate::mir::Terminator::new(
                            crate::mir::TerminatorKind::Call {
                                func: func_op,
                                args: vec![obj_op, key_arg, val_arg],
                                destination: Place::new(dummy_dest),
                                target: Some(target_bb),
                            },
                            expr.span,
                        ));
                        ctx.set_current_block(target_bb);

                        // Convert Move→Copy so lower_statement's result-temp
                        // creates a neutral IncRef/DecRef pair rather than a bare
                        // DecRef that would consume the RC donated to the map.
                        let ret_val = match val {
                            Operand::Move(p) => Operand::Copy(p),
                            other => other,
                        };
                        if let Some(d) = dest {
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(
                                    d.clone(),
                                    Rvalue::Use(ret_val.clone()),
                                ),
                                span: expr.span,
                            });
                            return Ok(Operand::Copy(d));
                        } else {
                            return Ok(ret_val);
                        }
                    }
                }

                let val = lower_expression(ctx, rhs, None)?;

                // Get the object operand
                let obj_operand = lower_expression(ctx, obj, None)?;

                let obj_place = ensure_place(ctx, obj_operand, obj.span);

                // Lower index expression
                let index_operand = lower_expression(ctx, idx, None)?;

                // Ensure index is in a local
                let index_local = match index_operand {
                    Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
                    _ => {
                        let ty = ctx
                            .type_checker
                            .get_type(idx.id)
                            .cloned()
                            .unwrap_or_else(|| Type::new(TypeKind::Int, idx.span));
                        let temp = ctx.push_temp(ty, idx.span);
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(
                                Place::new(temp),
                                Rvalue::Use(index_operand),
                            ),
                            span: idx.span,
                        });
                        temp
                    }
                };

                // Create indexed place
                let mut target_place = obj_place;
                target_place.projection.push(PlaceElem::Index(index_local));

                // Handle simple assignment vs compound assignment
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
                        let bin_op = match op {
                            crate::ast::operator::AssignmentOp::AssignAdd => BinOp::Add,
                            crate::ast::operator::AssignmentOp::AssignSub => BinOp::Sub,
                            crate::ast::operator::AssignmentOp::AssignMul => BinOp::Mul,
                            crate::ast::operator::AssignmentOp::AssignDiv => BinOp::Div,
                            crate::ast::operator::AssignmentOp::AssignMod => BinOp::Rem,
                            _ => unreachable!(),
                        };

                        let lhs_op = Operand::Copy(target_place.clone());
                        let result_ty = ctx
                            .type_checker
                            .get_type(index_expr.id)
                            .cloned()
                            .unwrap_or_else(|| Type::new(TypeKind::Int, expr.span));
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
                                target_place,
                                Rvalue::Use(Operand::Copy(Place::new(temp))),
                            ),
                            span: expr.span,
                        });
                    }
                }
                if let Some(d) = dest {
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(val.clone())),
                        span: expr.span,
                    });
                    return Ok(Operand::Copy(d));
                } else {
                    return Ok(val);
                }
            } else {
                return Err(LoweringError::unsupported_lhs(
                    "Expected Index expression",
                    expr.span,
                ));
            }
        }
    }
}
