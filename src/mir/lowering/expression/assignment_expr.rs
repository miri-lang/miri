// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{BinOp, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind};

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

                            // Before overwriting a managed local, DecRef the old value so
                            // it is released. The codegen's null guard makes this safe even
                            // on the first assignment (null pointer → no-op DecRef).
                            // Skip self-assignment (l = l) to avoid freeing a live object.
                            let is_self_copy = matches!(
                                &rvalue,
                                Rvalue::Use(Operand::Copy(p)) if p.local == local
                            );
                            if ctx.is_perceus_managed(&lhs_ty.kind) && !is_self_copy {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::DecRef(Place::new(local)),
                                    span: expr.span,
                                });
                            }

                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(Place::new(local), rvalue),
                                span: expr.span,
                            });
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
                                def.fields.iter().position(|(f, _)| f == field_name)
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
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        target_place,
                                        Rvalue::Use(val.clone()),
                                    ),
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
                    if matches!(&obj_ty.kind, TypeKind::Map(_, _))
                        || matches!(&obj_ty.kind, TypeKind::Custom(name, _) if name == "Map")
                    {
                        let val = lower_expression(ctx, rhs, None)?;
                        let obj_op = lower_expression(ctx, obj, None)?;
                        let key_op = lower_expression(ctx, idx, None)?;

                        let func_op = Operand::Constant(Box::new(crate::mir::Constant {
                            span: expr.span,
                            ty: Type::new(TypeKind::Identifier, expr.span),
                            literal: crate::ast::literal::Literal::Identifier(
                                "miri_rt_map_set".to_string(),
                            ),
                        }));

                        let target_bb = ctx.new_basic_block();
                        let dummy_dest =
                            ctx.push_temp(Type::new(TypeKind::Void, expr.span), expr.span);

                        ctx.set_terminator(crate::mir::Terminator::new(
                            crate::mir::TerminatorKind::Call {
                                func: func_op,
                                args: vec![obj_op, key_op, val.clone()],
                                destination: Place::new(dummy_dest),
                                target: Some(target_bb),
                            },
                            expr.span,
                        ));
                        ctx.set_current_block(target_bb);

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
