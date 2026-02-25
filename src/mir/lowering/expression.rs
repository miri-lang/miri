// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::Pattern;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::lambda::{CapturedVar, LambdaInfo};
use crate::mir::{
    AggregateKind, BinOp, Body, Constant, Dimension, Discriminant, ExecutionModel, GpuIntrinsic,
    LocalDecl, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind, UnOp,
};

use super::context::LoweringContext;
use super::control_flow::lower_call;
use super::helpers::{
    bind_pattern, literal_to_u128, lower_as_return, lower_to_local, resolve_type,
};
use super::statement::lower_statement;

pub fn lower_expression(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    match &expr.node {
        ExpressionKind::Literal(lit) => {
            // Prefer type checker's resolved type for proper context-aware typing
            // Only infer from literal if type checker doesn't have a type
            let ty = if let Some(resolved) = ctx.type_checker.get_type(expr.id) {
                resolved.clone()
            } else {
                match lit {
                    crate::ast::literal::Literal::Integer(int_lit) => {
                        // Preserve specific integer type from the literal
                        use crate::ast::literal::IntegerLiteral;
                        match int_lit {
                            IntegerLiteral::I8(_) => Type::new(TypeKind::I8, expr.span),
                            IntegerLiteral::I16(_) => Type::new(TypeKind::I16, expr.span),
                            IntegerLiteral::I32(_) => Type::new(TypeKind::I32, expr.span),
                            IntegerLiteral::I64(_) => Type::new(TypeKind::I64, expr.span),
                            IntegerLiteral::I128(_) => Type::new(TypeKind::I128, expr.span),
                            IntegerLiteral::U8(_) => Type::new(TypeKind::U8, expr.span),
                            IntegerLiteral::U16(_) => Type::new(TypeKind::U16, expr.span),
                            IntegerLiteral::U32(_) => Type::new(TypeKind::U32, expr.span),
                            IntegerLiteral::U64(_) => Type::new(TypeKind::U64, expr.span),
                            IntegerLiteral::U128(_) => Type::new(TypeKind::U128, expr.span),
                        }
                    }
                    crate::ast::literal::Literal::Boolean(_) => {
                        Type::new(TypeKind::Boolean, expr.span)
                    }
                    crate::ast::literal::Literal::String(_) => {
                        Type::new(TypeKind::String, expr.span)
                    }
                    crate::ast::literal::Literal::Float(float_lit) => {
                        // Preserve specific float type from the literal
                        use crate::ast::literal::FloatLiteral;
                        match float_lit {
                            FloatLiteral::F32(_) => Type::new(TypeKind::F32, expr.span),
                            FloatLiteral::F64(_) => Type::new(TypeKind::F64, expr.span),
                        }
                    }
                    crate::ast::literal::Literal::Symbol(_) => {
                        Type::new(TypeKind::Symbol, expr.span)
                    }
                    crate::ast::literal::Literal::Regex(_) => {
                        // Regex literals are represented as strings internally
                        Type::new(TypeKind::String, expr.span)
                    }
                    crate::ast::literal::Literal::None => {
                        // None represents the absence of a value (null/nil)
                        // Use Void type since it's the unit type in Miri
                        Type::new(TypeKind::Void, expr.span)
                    }
                }
            };

            let constant = Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty,
                literal: lit.clone(),
            }));

            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(constant.clone())),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                Ok(constant)
            }
        }
        ExpressionKind::Identifier(name, _) => {
            if let Some(&local) = ctx.variable_map.get(name.as_str()) {
                // If destination is provided, assign the variable to it
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
                    // Check if the type is Copy to determine Move vs Copy semantics
                    let ty = &ctx.body.local_decls[local.0].ty;
                    if ty.is_copy() {
                        Ok(Operand::Copy(Place::new(local)))
                    } else {
                        Ok(Operand::Move(Place::new(local)))
                    }
                }
            } else {
                // Assume global function/symbol
                // In a real compiler we would check if it exists in globals
                let constant = Operand::Constant(Box::new(Constant {
                    span: expr.span,
                    ty: Type::new(TypeKind::Symbol, expr.span),
                    literal: crate::ast::literal::Literal::Symbol(name.clone()),
                }));

                if let Some(d) = dest {
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(constant.clone())),
                        span: expr.span,
                    });
                    Ok(Operand::Copy(d))
                } else {
                    Ok(constant)
                }
            }
        }
        ExpressionKind::Assignment(lhs, op, rhs) => {
            match &**lhs {
                crate::ast::expression::LeftHandSideExpression::Identifier(id_expr) => {
                    if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                        let val = lower_expression(ctx, rhs, None)?;

                        if let Some(&local) = ctx.variable_map.get(name.as_str()) {
                            match op {
                                crate::ast::operator::AssignmentOp::Assign => {
                                    let lhs_ty = ctx.body.local_decls[local.0].ty.clone();
                                    let rhs_ty = val.ty(&ctx.body);

                                    let rvalue = if rhs_ty.kind != lhs_ty.kind {
                                        Rvalue::Cast(Box::new(val.clone()), lhs_ty)
                                    } else {
                                        Rvalue::Use(val.clone())
                                    };

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
                                    kind: MirStatementKind::Assign(
                                        d.clone(),
                                        Rvalue::Use(val.clone()),
                                    ),
                                    span: expr.span,
                                });
                                Ok(Operand::Copy(d))
                            } else {
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

                        if let TypeKind::Custom(struct_name, _) = &obj_ty.kind {
                            if let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
                                ctx.type_checker.global_type_definitions.get(struct_name)
                            {
                                if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                                    if let Some(idx) =
                                        def.fields.iter().position(|(f, _, _)| f == field_name)
                                    {
                                        // Ensure obj is a Place
                                        let obj_place = match obj_operand {
                                            Operand::Copy(p) | Operand::Move(p) => p,
                                            Operand::Constant(c) => {
                                                let temp = ctx.push_temp(c.ty.clone(), obj.span);
                                                ctx.push_statement(crate::mir::Statement {
                                                    kind: MirStatementKind::Assign(
                                                        Place::new(temp),
                                                        Rvalue::Use(Operand::Constant(c)),
                                                    ),
                                                    span: obj.span,
                                                });
                                                Place::new(temp)
                                            }
                                        };

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
                                                let result_ty =
                                                    resolve_type(ctx.type_checker, prop);
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
                                                        Rvalue::Use(Operand::Copy(Place::new(
                                                            temp,
                                                        ))),
                                                    ),
                                                    span: expr.span,
                                                });
                                            }
                                        }
                                        if let Some(d) = dest {
                                            ctx.push_statement(crate::mir::Statement {
                                                kind: MirStatementKind::Assign(
                                                    d.clone(),
                                                    Rvalue::Use(val.clone()),
                                                ),
                                                span: expr.span,
                                            });
                                            return Ok(Operand::Copy(d));
                                        } else {
                                            return Ok(val);
                                        }
                                    }
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
                        let val = lower_expression(ctx, rhs, None)?;

                        // Get the object operand
                        let obj_operand = lower_expression(ctx, obj, None)?;

                        // Ensure obj is a Place
                        let obj_place = match obj_operand {
                            Operand::Copy(p) | Operand::Move(p) => p,
                            Operand::Constant(c) => {
                                let temp = ctx.push_temp(c.ty.clone(), obj.span);
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Use(Operand::Constant(c)),
                                    ),
                                    span: obj.span,
                                });
                                Place::new(temp)
                            }
                        };

                        // Lower index expression
                        let index_operand = lower_expression(ctx, idx, None)?;

                        // Ensure index is in a local
                        let index_local = match index_operand {
                            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                                p.local
                            }
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
                                let result_ty = ctx
                                    .type_checker
                                    .get_type(index_expr.id)
                                    .cloned()
                                    .unwrap_or_else(|| Type::new(TypeKind::Int, expr.span));
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
                    } else {
                        return Err(LoweringError::unsupported_lhs(
                            "Expected Index expression",
                            expr.span,
                        ));
                    }
                }
            }
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            // Handle `In` operator specially - it's a membership test
            if matches!(op, crate::ast::operator::BinaryOp::In) {
                let lhs_op = lower_expression(ctx, lhs, None)?;
                let rhs_op = lower_expression(ctx, rhs, None)?;

                // For now, implement as a call to built-in `contains` function
                // Backend will handle the actual membership test
                let result_ty = Type::new(TypeKind::Boolean, expr.span);
                let temp = ctx.push_temp(result_ty, expr.span);

                // Create call to __contains(collection, element) -> bool
                let contains_fn = Operand::Constant(Box::new(Constant {
                    span: expr.span,
                    ty: Type::new(TypeKind::Symbol, expr.span),
                    literal: crate::ast::literal::Literal::Symbol("__contains".to_string()),
                }));

                let target_bb = ctx.new_basic_block();
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Call {
                        func: contains_fn,
                        args: vec![rhs_op, lhs_op], // (collection, element)
                        destination: Place::new(temp),
                        target: Some(target_bb),
                    },
                    expr.span,
                ));
                ctx.set_current_block(target_bb);

                return Ok(Operand::Copy(Place::new(temp)));
            }

            let lhs_op = lower_expression(ctx, lhs, None)?;
            let rhs_op = lower_expression(ctx, rhs, None)?;

            // Trait-based binary operator dispatch for class types.
            // Maps operators to trait methods:
            //   BinaryOp::Add     → Addable::concat
            //   BinaryOp::Equal   → Equatable::equals
            //   BinaryOp::NotEqual → NOT Equatable::equals
            if let Some(lhs_ty) = ctx.type_checker.get_type(lhs.id) {
                let class_name = match &lhs_ty.kind {
                    TypeKind::String => Some("String".to_string()),
                    TypeKind::Custom(name, _) => Some(name.clone()),
                    _ => None,
                };

                if let Some(class_name) = class_name {
                    // Map operator to (trait_name, method_name, is_negated)
                    let op_mapping = match op {
                        crate::ast::operator::BinaryOp::Add => Some(("Addable", "concat", false)),
                        crate::ast::operator::BinaryOp::Mul => {
                            Some(("Multiplicable", "repeat", false))
                        }
                        crate::ast::operator::BinaryOp::Equal => {
                            Some(("Equatable", "equals", false))
                        }
                        crate::ast::operator::BinaryOp::NotEqual => {
                            Some(("Equatable", "equals", true))
                        }
                        _ => None,
                    };

                    if let Some((_trait_name, method_name, negate)) = op_mapping {
                        if let Some(crate::type_checker::context::TypeDefinition::Class(
                            class_def,
                        )) = ctx.type_checker.global_type_definitions.get(&class_name)
                        {
                            if class_def.methods.contains_key(method_name) {
                                use crate::ast::literal::Literal;

                                let mangled_name = format!("{}_{}", class_name, method_name);

                                let alloc_op = ctx
                                    .variable_map
                                    .get("allocator")
                                    .map(|&al| Operand::Copy(Place::new(al)));

                                let mut call_args = vec![lhs_op, rhs_op];
                                if let Some(alloc) = alloc_op {
                                    call_args.push(alloc);
                                }

                                let method_info = &class_def.methods[method_name];
                                let return_ty = method_info.return_type.clone();

                                let func_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span,
                                    ty: Type::new(TypeKind::Symbol, expr.span),
                                    literal: Literal::Symbol(mangled_name),
                                }));

                                if negate {
                                    // NotEqual: call equals then NOT the result
                                    let eq_temp = ctx.push_temp(return_ty.clone(), expr.span);
                                    let after_eq_bb = ctx.new_basic_block();
                                    ctx.set_terminator(Terminator::new(
                                        TerminatorKind::Call {
                                            func: func_op,
                                            args: call_args,
                                            destination: Place::new(eq_temp),
                                            target: Some(after_eq_bb),
                                        },
                                        expr.span,
                                    ));
                                    ctx.set_current_block(after_eq_bb);

                                    let (target, ret_op) = if let Some(d) = dest {
                                        (d.clone(), Operand::Copy(d))
                                    } else {
                                        let temp = ctx.push_temp(return_ty, expr.span);
                                        (Place::new(temp), Operand::Copy(Place::new(temp)))
                                    };
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            target,
                                            Rvalue::UnaryOp(
                                                UnOp::Not,
                                                Box::new(Operand::Copy(Place::new(eq_temp))),
                                            ),
                                        ),
                                        span: expr.span,
                                    });
                                    return Ok(ret_op);
                                }

                                // Normal case: call the method directly
                                let (destination, ret_op) = if let Some(d) = dest {
                                    (d.clone(), Operand::Copy(d))
                                } else {
                                    let temp = ctx.push_temp(return_ty, expr.span);
                                    let p = Place::new(temp);
                                    (p.clone(), Operand::Copy(p))
                                };
                                let target_bb = ctx.new_basic_block();
                                ctx.set_terminator(Terminator::new(
                                    TerminatorKind::Call {
                                        func: func_op,
                                        args: call_args,
                                        destination,
                                        target: Some(target_bb),
                                    },
                                    expr.span,
                                ));
                                ctx.set_current_block(target_bb);
                                return Ok(ret_op);
                            }
                        }
                    }
                }
            }

            let bin_op = match op {
                crate::ast::operator::BinaryOp::Add => BinOp::Add,
                crate::ast::operator::BinaryOp::Sub => BinOp::Sub,
                crate::ast::operator::BinaryOp::Mul => BinOp::Mul,
                crate::ast::operator::BinaryOp::Div => BinOp::Div,
                crate::ast::operator::BinaryOp::Mod => BinOp::Rem,
                crate::ast::operator::BinaryOp::BitwiseAnd => BinOp::BitAnd,
                crate::ast::operator::BinaryOp::BitwiseOr => BinOp::BitOr,
                crate::ast::operator::BinaryOp::BitwiseXor => BinOp::BitXor,
                crate::ast::operator::BinaryOp::Equal => BinOp::Eq,
                crate::ast::operator::BinaryOp::NotEqual => BinOp::Ne,
                crate::ast::operator::BinaryOp::LessThan => BinOp::Lt,
                crate::ast::operator::BinaryOp::LessThanEqual => BinOp::Le,
                crate::ast::operator::BinaryOp::GreaterThan => BinOp::Gt,
                crate::ast::operator::BinaryOp::GreaterThanEqual => BinOp::Ge,
                // Range and In are handled separately, And/Or via Logical
                _ => {
                    return Err(LoweringError::unsupported_operator(
                        format!("{:?}", op),
                        expr.span,
                    ));
                }
            };

            let result_ty = match op {
                crate::ast::operator::BinaryOp::Equal
                | crate::ast::operator::BinaryOp::NotEqual
                | crate::ast::operator::BinaryOp::LessThan
                | crate::ast::operator::BinaryOp::LessThanEqual
                | crate::ast::operator::BinaryOp::GreaterThan
                | crate::ast::operator::BinaryOp::GreaterThanEqual => {
                    Type::new(TypeKind::Boolean, expr.span)
                }
                _ => match &lhs_op {
                    Operand::Constant(c) => c.ty.clone(),
                    Operand::Copy(place) | Operand::Move(place) => {
                        ctx.body.local_decls[place.local.0].ty.clone()
                    }
                },
            };

            let (target, ret_op) = if let Some(d) = dest {
                (d.clone(), Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(result_ty, expr.span);
                (Place::new(temp), Operand::Copy(Place::new(temp)))
            };

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    target,
                    Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(rhs_op)),
                ),
                span: expr.span,
            });

            Ok(ret_op)
        }
        ExpressionKind::Unary(op, operand) => {
            let op_val = lower_expression(ctx, operand, None)?;
            let un_op = match op {
                crate::ast::operator::UnaryOp::Negate => UnOp::Neg,
                crate::ast::operator::UnaryOp::Not => UnOp::Not,
                crate::ast::operator::UnaryOp::Await => UnOp::Await,
                // Decrement (--x) is treated as double negation: -(-x) = x
                // We recursively lower the operand and then negate twice
                crate::ast::operator::UnaryOp::Decrement => {
                    // First negate
                    let first_neg_ty = match &op_val {
                        Operand::Constant(c) => c.ty.clone(),
                        Operand::Copy(place) | Operand::Move(place) => {
                            ctx.body.local_decls[place.local.0].ty.clone()
                        }
                    };
                    let first_neg = ctx.push_temp(first_neg_ty.clone(), expr.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(first_neg),
                            Rvalue::UnaryOp(UnOp::Neg, Box::new(op_val)),
                        ),
                        span: expr.span,
                    });

                    // Second negate
                    let second_neg = ctx.push_temp(first_neg_ty, expr.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(second_neg),
                            Rvalue::UnaryOp(
                                UnOp::Neg,
                                Box::new(Operand::Copy(Place::new(first_neg))),
                            ),
                        ),
                        span: expr.span,
                    });

                    return Ok(Operand::Copy(Place::new(second_neg)));
                }
                // Increment (++x) is a no-op for value (not implemented as mutation)
                crate::ast::operator::UnaryOp::Increment => {
                    return Ok(op_val);
                }
                // Plus is identity
                crate::ast::operator::UnaryOp::Plus => {
                    return Ok(op_val);
                }
                // BitwiseNot - similar to Not
                crate::ast::operator::UnaryOp::BitwiseNot => UnOp::Not,
            };

            let result_ty = match &op_val {
                Operand::Constant(c) => c.ty.clone(),
                Operand::Copy(place) | Operand::Move(place) => {
                    ctx.body.local_decls[place.local.0].ty.clone()
                }
            };

            let (target, ret_op) = if let Some(d) = dest {
                (d.clone(), Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(result_ty, expr.span);
                (Place::new(temp), Operand::Copy(Place::new(temp)))
            };

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(target, Rvalue::UnaryOp(un_op, Box::new(op_val))),
                span: expr.span,
            });

            Ok(ret_op)
        }
        ExpressionKind::Call(func, args) => {
            // Check for legacy GPU intrinsic function names (gpu_thread_idx_x etc.)
            if let ExpressionKind::Identifier(name, _) = &func.node {
                let intrinsic_rvalue = match name.as_str() {
                    "gpu_thread_idx_x" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)))
                    }
                    "gpu_thread_idx_y" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::Y)))
                    }
                    "gpu_thread_idx_z" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::Z)))
                    }
                    "gpu_block_idx_x" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::X)))
                    }
                    "gpu_block_idx_y" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Y)))
                    }
                    "gpu_block_idx_z" => {
                        Some(Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Z)))
                    }
                    _ => None,
                };

                if let Some(rvalue) = intrinsic_rvalue {
                    let temp = ctx.push_temp(Type::new(TypeKind::Int, expr.span), expr.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(Place::new(temp), rvalue),
                        span: expr.span,
                    });
                    return Ok(Operand::Copy(Place::new(temp)));
                }
            }

            // Check for enum variant with associated values (e.g., Event.Click(1, 2))
            if let ExpressionKind::Member(enum_expr, variant_expr) = &func.node {
                if let ExpressionKind::Identifier(type_name, _) = &enum_expr.node {
                    if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                        ctx.type_checker.global_type_definitions.get(type_name)
                    {
                        if let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node {
                            if let Some((discriminant, _)) = enum_def
                                .variants
                                .iter()
                                .enumerate()
                                .find(|(_, (name, _))| name.as_str() == variant_name)
                            {
                                // Variant with associated values.
                                // Create discriminant constant
                                let discr_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span,
                                    ty: Type::new(TypeKind::Int, expr.span),
                                    literal: crate::ast::literal::Literal::Integer(
                                        crate::ast::literal::IntegerLiteral::I32(
                                            discriminant as i32,
                                        ),
                                    ),
                                }));

                                // Lower all arguments
                                let mut ops = vec![discr_op];
                                for arg in args {
                                    ops.push(lower_expression(ctx, arg, None)?);
                                }

                                // DPS: use the caller-provided destination if given,
                                // otherwise allocate a fresh temp.
                                let target = if let Some(d) = dest {
                                    d
                                } else {
                                    let ty = resolve_type(ctx.type_checker, expr);
                                    Place::new(ctx.push_temp(ty, expr.span))
                                };

                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        target.clone(),
                                        Rvalue::Aggregate(
                                            AggregateKind::Enum(
                                                type_name.clone(),
                                                variant_name.clone(),
                                            ),
                                            ops,
                                        ),
                                    ),
                                    span: expr.span,
                                });
                                return Ok(Operand::Copy(target));
                            }
                        }
                    }
                }
            }

            lower_call(ctx, &expr.span, expr.id, func, args, dest)
        }
        ExpressionKind::Member(obj, prop) => {
            let obj_operand = lower_expression(ctx, obj, None)?;

            // Handle GPU Intrinsics (gpu_context.thread_idx.x, etc.)
            // This uses a two-step lowering: gpu_context.thread_idx => intermediate symbol,
            // then intermediate_symbol.x => actual GpuIntrinsic rvalue.
            if let Operand::Constant(c) = &obj_operand {
                if let crate::ast::literal::Literal::Symbol(sym) = &c.literal {
                    if sym == "gpu_context" {
                        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                            // Return intermediate symbol for chained access
                            return Ok(Operand::Constant(Box::new(Constant {
                                span: expr.span,
                                ty: Type::new(TypeKind::Void, expr.span),
                                literal: crate::ast::literal::Literal::Symbol(format!(
                                    "gpu_context.{}",
                                    prop_name
                                )),
                            })));
                        }
                    } else if sym.starts_with("gpu_context.") {
                        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                            let dim = match prop_name.as_str() {
                                "x" => Dimension::X,
                                "y" => Dimension::Y,
                                "z" => Dimension::Z,
                                _ => {
                                    return Err(LoweringError::unsupported_expression(
                                        format!("Invalid GPU dimension: {}", prop_name),
                                        expr.span,
                                    ));
                                }
                            };

                            let rvalue = match sym.as_str() {
                                "gpu_context.thread_idx" => {
                                    Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(dim))
                                }
                                "gpu_context.block_idx" => {
                                    Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(dim))
                                }
                                "gpu_context.block_dim" => {
                                    Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(dim))
                                }
                                "gpu_context.grid_dim" => {
                                    Rvalue::GpuIntrinsic(GpuIntrinsic::GridDim(dim))
                                }
                                _ => {
                                    return Err(LoweringError::unsupported_expression(
                                        format!("Unknown GPU intrinsic: {}", sym),
                                        expr.span,
                                    ));
                                }
                            };

                            match &dest {
                                Some(d) => {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(d.clone(), rvalue),
                                        span: expr.span,
                                    });
                                    return Ok(Operand::Copy(d.clone()));
                                }
                                None => {
                                    let temp = ctx
                                        .push_temp(Type::new(TypeKind::Int, expr.span), expr.span);
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(Place::new(temp), rvalue),
                                        span: expr.span,
                                    });
                                    return Ok(Operand::Copy(Place::new(temp)));
                                }
                            }
                        }
                    }
                }
            }

            // 2. Handle General Struct Member Access
            let obj_ty = if let Some(ty) = ctx.type_checker.get_type(obj.id) {
                ty
            } else {
                return Err(LoweringError::type_not_found(obj.id, expr.span));
            };

            // Handle Tuple Member Access
            if let TypeKind::Tuple(elements) = &obj_ty.kind {
                if let ExpressionKind::Literal(crate::ast::literal::Literal::Integer(val)) =
                    &prop.node
                {
                    let idx = match val {
                        crate::ast::literal::IntegerLiteral::I8(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::I16(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::I32(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::I64(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::I128(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::U8(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::U16(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::U32(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::U64(v) => *v as usize,
                        crate::ast::literal::IntegerLiteral::U128(v) => *v as usize,
                    };

                    // Ensure obj is a Place
                    let obj_place = match obj_operand {
                        Operand::Copy(p) | Operand::Move(p) => p,
                        Operand::Constant(c) => {
                            let temp = ctx.push_temp(c.ty.clone(), obj.span);
                            ctx.push_statement(crate::mir::Statement {
                                kind: MirStatementKind::Assign(
                                    Place::new(temp),
                                    Rvalue::Use(Operand::Constant(c)),
                                ),
                                span: obj.span,
                            });
                            Place::new(temp)
                        }
                    };

                    let mut target_place = obj_place;
                    target_place.projection.push(PlaceElem::Field(idx));

                    let element_ty = resolve_type(ctx.type_checker, &elements[idx]);

                    let operand = if element_ty.is_copy() {
                        Operand::Copy(target_place.clone())
                    } else {
                        Operand::Move(target_place.clone())
                    };

                    if let Some(d) = dest {
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(operand)),
                            span: expr.span,
                        });
                        return Ok(Operand::Copy(d));
                    } else {
                        return Ok(operand);
                    }
                }
            }

            if let TypeKind::Custom(struct_name, _) = &obj_ty.kind {
                // Find field index
                // We need to look up the struct definition in the type checker.
                // The type checker doesn't expose a direct "get_field_index" method,
                // but we can look up the definition.
                // Note: Global type definitions are available.
                if let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(struct_name)
                {
                    if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                        if let Some(idx) = def.fields.iter().position(|(f, _, _)| f == field_name) {
                            // Ensure obj is a Place (copy to temp if constant)
                            let place = match obj_operand {
                                Operand::Copy(p) | Operand::Move(p) => p,
                                Operand::Constant(c) => {
                                    let temp = ctx.push_temp(c.ty.clone(), obj.span);
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            Place::new(temp),
                                            Rvalue::Use(Operand::Constant(c)),
                                        ),
                                        span: obj.span,
                                    });
                                    Place::new(temp)
                                }
                            };

                            // Create new place with projection
                            let mut new_place = place.clone();
                            new_place.projection.push(PlaceElem::Field(idx));

                            if let Some(d) = dest {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        d.clone(),
                                        Rvalue::Use(Operand::Copy(new_place)),
                                    ),
                                    span: expr.span,
                                });
                                return Ok(Operand::Copy(d));
                            } else {
                                return Ok(Operand::Copy(new_place));
                            }
                        } else {
                            return Err(LoweringError::unsupported_lhs(
                                format!(
                                    "Field '{}' not found in struct '{}'",
                                    field_name, struct_name
                                ),
                                obj.span,
                            ));
                        }
                    }
                }

                // Also check for class field access
                if let Some(crate::type_checker::context::TypeDefinition::Class(def)) =
                    ctx.type_checker.global_type_definitions.get(struct_name)
                {
                    if let ExpressionKind::Identifier(field_name, _) = &prop.node {
                        // Check fields in BTreeMap (note: index is stored in FieldInfo)
                        if let Some((idx, _)) = def
                            .fields
                            .iter()
                            .enumerate()
                            .find(|(_, (f, _))| *f == field_name)
                        {
                            // Ensure obj is a Place (copy to temp if constant)
                            let place = match obj_operand {
                                Operand::Copy(p) | Operand::Move(p) => p,
                                Operand::Constant(c) => {
                                    let temp = ctx.push_temp(c.ty.clone(), obj.span);
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            Place::new(temp),
                                            Rvalue::Use(Operand::Constant(c)),
                                        ),
                                        span: obj.span,
                                    });
                                    Place::new(temp)
                                }
                            };

                            // Create new place with projection
                            let mut new_place = place.clone();
                            new_place.projection.push(PlaceElem::Field(idx));

                            if let Some(d) = dest {
                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        d.clone(),
                                        Rvalue::Use(Operand::Copy(new_place)),
                                    ),
                                    span: expr.span,
                                });
                                return Ok(Operand::Copy(d));
                            } else {
                                return Ok(Operand::Copy(new_place));
                            }
                        }
                        // If field not found, might be a method call, which is handled in Call
                    }
                }
            }

            // 3. Handle Enum Unit Variant Access (e.g., Status.Ok)
            // Check if obj is a type identifier and prop is an enum variant
            if let ExpressionKind::Identifier(type_name, _) = &obj.node {
                if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                    ctx.type_checker.global_type_definitions.get(type_name)
                {
                    if let ExpressionKind::Identifier(variant_name, _) = &prop.node {
                        if let Some((discriminant, _)) = enum_def
                            .variants
                            .iter()
                            .enumerate()
                            .find(|(_, (name, _))| name.as_str() == variant_name)
                        {
                            // Unit variant: create Aggregate with just discriminant
                            let associated_types = match enum_def.variants.get(variant_name) {
                                Some(types) => types,
                                None => {
                                    return Err(LoweringError::unsupported_expression(
                                        format!(
                                            "Unknown variant '{}' for enum '{}'",
                                            variant_name, type_name
                                        ),
                                        expr.span,
                                    ));
                                }
                            };
                            if associated_types.is_empty() {
                                let ty = resolve_type(ctx.type_checker, expr);

                                // Create discriminant constant
                                let discr_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span,
                                    ty: Type::new(TypeKind::Int, expr.span),
                                    literal: crate::ast::literal::Literal::Integer(
                                        crate::ast::literal::IntegerLiteral::I32(
                                            discriminant as i32,
                                        ),
                                    ),
                                }));

                                if let Some(d) = dest {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            d.clone(),
                                            Rvalue::Aggregate(
                                                AggregateKind::Enum(
                                                    type_name.clone(),
                                                    variant_name.clone(),
                                                ),
                                                vec![discr_op],
                                            ),
                                        ),
                                        span: expr.span,
                                    });
                                    return Ok(Operand::Copy(d));
                                } else {
                                    let temp = ctx.push_temp(ty, expr.span);
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            Place::new(temp),
                                            Rvalue::Aggregate(
                                                AggregateKind::Enum(
                                                    type_name.clone(),
                                                    variant_name.clone(),
                                                ),
                                                vec![discr_op],
                                            ),
                                        ),
                                        span: expr.span,
                                    });
                                    return Ok(Operand::Copy(Place::new(temp)));
                                }
                            }
                            // Variant with associated values - handled in Call
                        }
                    }
                }
            }

            // Handle class method-as-property access (e.g. s.length, s.size).
            // Zero-arg methods on class types can be accessed as properties.
            let class_name = match &obj_ty.kind {
                TypeKind::String => Some("String".to_string()),
                TypeKind::Custom(name, _) => Some(name.clone()),
                _ => None,
            };

            if let Some(class_name) = class_name {
                if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                    if let Some(crate::type_checker::context::TypeDefinition::Class(class_def)) =
                        ctx.type_checker.global_type_definitions.get(&class_name)
                    {
                        if let Some(method_info) = class_def.methods.get(prop_name.as_str()) {
                            // Only treat zero-arg methods as property access
                            if method_info.params.is_empty() {
                                let mangled_name = format!("{}_{}", class_name, prop_name);
                                let return_ty = method_info.return_type.clone();

                                let func_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span,
                                    ty: crate::ast::types::Type::new(TypeKind::Symbol, expr.span),
                                    literal: crate::ast::literal::Literal::Symbol(mangled_name),
                                }));

                                let mut call_args = vec![obj_operand];
                                if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
                                    call_args.push(Operand::Copy(Place::new(alloc_local)));
                                }

                                let (destination, op) = if let Some(d) = dest {
                                    (d.clone(), Operand::Copy(d))
                                } else {
                                    let temp = ctx.push_temp(return_ty, expr.span);
                                    let p = Place::new(temp);
                                    (p.clone(), Operand::Copy(p))
                                };

                                let target_bb = ctx.new_basic_block();
                                ctx.set_terminator(Terminator::new(
                                    TerminatorKind::Call {
                                        func: func_op,
                                        args: call_args,
                                        destination,
                                        target: Some(target_bb),
                                    },
                                    expr.span,
                                ));
                                ctx.set_current_block(target_bb);
                                return Ok(op);
                            }
                        }
                    }
                }
            }

            Err(LoweringError::unsupported_expression(
                format!("Unsupported member access on type: {}", obj_ty),
                expr.span,
            ))
        }
        ExpressionKind::Tuple(elements) => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expression(ctx, e, None))
                .collect::<Result<_, _>>()?;
            let ty = resolve_type(ctx.type_checker, expr);
            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Aggregate(AggregateKind::Tuple, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(ty, expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::Tuple, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(Place::new(temp)))
            }
        }
        ExpressionKind::List(elements) => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expression(ctx, e, None))
                .collect::<Result<_, _>>()?;
            let ty = resolve_type(ctx.type_checker, expr);
            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Aggregate(AggregateKind::List, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(ty, expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::List, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(Place::new(temp)))
            }
        }
        ExpressionKind::Array(elements, _size) => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expression(ctx, e, None))
                .collect::<Result<_, _>>()?;
            let ty = resolve_type(ctx.type_checker, expr);
            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Aggregate(AggregateKind::Array, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(ty, expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::Array, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(Place::new(temp)))
            }
        }
        /*
        ExpressionKind::Struct(elements, _) => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expression(ctx, e))
                .collect::<Result<_, _>>()?;
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Struct(ty.clone()), ops),
                ),
                span: expr.span,
            });
            Ok(Operand::Copy(Place::new(temp)))
        }
        */
        ExpressionKind::Set(elements) => {
            let ops: Vec<Operand> = elements
                .iter()
                .map(|e| lower_expression(ctx, e, None))
                .collect::<Result<_, _>>()?;
            let ty = resolve_type(ctx.type_checker, expr);
            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Aggregate(AggregateKind::Set, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(ty, expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::Set, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(Place::new(temp)))
            }
        }
        ExpressionKind::Map(pairs) => {
            // Flatten pairs into [key1, val1, key2, val2, ...]
            let mut ops: Vec<Operand> = Vec::with_capacity(pairs.len() * 2);
            for (key, val) in pairs {
                ops.push(lower_expression(ctx, key, None)?);
                ops.push(lower_expression(ctx, val, None)?);
            }
            let ty = resolve_type(ctx.type_checker, expr);
            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Aggregate(AggregateKind::Map, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                let temp = ctx.push_temp(ty, expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Aggregate(AggregateKind::Map, ops),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(Place::new(temp)))
            }
        }
        ExpressionKind::Index(obj, index_expr) => {
            // Lower object to get a place
            let obj_operand = lower_expression(ctx, obj, None)?;

            // Ensure object is in a place (copy to temp if constant)
            let obj_place = match obj_operand {
                Operand::Copy(p) | Operand::Move(p) => p,
                Operand::Constant(c) => {
                    let temp = ctx.push_temp(c.ty.clone(), obj.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Use(Operand::Constant(c)),
                        ),
                        span: obj.span,
                    });
                    Place::new(temp)
                }
            };

            // Lower index expression
            let index_operand = lower_expression(ctx, index_expr, None)?;

            // Ensure index is in a local (PlaceElem::Index requires Local)
            let index_local = match index_operand {
                Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
                _ => {
                    // Store in temp - use inferred type from type checker or default to Int
                    let ty = ctx
                        .type_checker
                        .get_type(index_expr.id)
                        .cloned()
                        .unwrap_or_else(|| Type::new(TypeKind::Int, index_expr.span));
                    let temp = ctx.push_temp(ty, index_expr.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Use(index_operand),
                        ),
                        span: index_expr.span,
                    });
                    temp
                }
            };

            // Create indexed place with projection
            let mut indexed_place = obj_place;
            indexed_place.projection.push(PlaceElem::Index(index_local));

            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Use(Operand::Copy(indexed_place)),
                    ),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                Ok(Operand::Copy(indexed_place))
            }
        }
        ExpressionKind::Match(subject, branches) => {
            // Lower the subject expression
            let subject_op = lower_expression(ctx, subject, None)?;

            // Store subject in a temp so we can reference it multiple times
            let subject_ty = resolve_type(ctx.type_checker, subject);
            let subject_local = ctx.push_temp(subject_ty.clone(), subject.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(subject_local), Rvalue::Use(subject_op)),
                span: subject.span,
            });

            // Use dest if provided (DPS), otherwise create a temp
            let result_ty = resolve_type(ctx.type_checker, expr);
            let result_local = if let Some(ref dest_place) = dest {
                dest_place.local
            } else {
                ctx.push_temp(result_ty.clone(), expr.span)
            };

            // Create join block where all branches converge
            let join_bb = ctx.new_basic_block();

            // Collect literal patterns for SwitchInt.
            // branch_blocks stores (block, branch, discriminants) where discriminants is
            // non-empty for arms with specific literal/enum patterns and empty for catch-all
            // arms (identifier, default, tuple, regex).  The discriminants are used when
            // computing guard-failure targets (see second pass below).
            //
            // IMPORTANT: only the *first* arm that covers a given discriminant value is
            // registered in switch_targets.  Subsequent arms with the same discriminant
            // (e.g. a guarded arm followed by an unguarded fallback for the same literal)
            // are reachable only via the guard-failure chain, NOT via a second SwitchInt
            // dispatch.  Adding duplicate discriminants to switch_targets causes the
            // Cranelift translator (which uses `.pop()` to build a brif chain in reverse)
            // to dispatch to the *last* duplicate first, bypassing any earlier guarded arm.
            let mut switch_targets: Vec<(Discriminant, crate::mir::block::BasicBlock)> = Vec::new();
            let mut seen_discrs: std::collections::HashSet<u128> = std::collections::HashSet::new();
            let mut otherwise_bb = None;
            let mut branch_blocks: Vec<(
                crate::mir::block::BasicBlock,
                &crate::ast::pattern::MatchBranch,
                Vec<u128>, // discriminants covered; empty ⇒ catch-all
            )> = Vec::new();

            for branch in branches {
                let branch_bb = ctx.new_basic_block();
                let mut arm_discrs: Vec<u128> = Vec::new();

                for pattern in &branch.patterns {
                    match pattern {
                        Pattern::Literal(lit) => {
                            if let Some(val) = literal_to_u128(lit) {
                                arm_discrs.push(val);
                                // Only register the first arm per discriminant in switch_targets.
                                if seen_discrs.insert(val) {
                                    switch_targets.push((Discriminant::from(val), branch_bb));
                                }
                            }
                        }
                        Pattern::Default => {
                            otherwise_bb = Some(branch_bb);
                        }
                        Pattern::Identifier(_) => {
                            // Identifier pattern matches everything - treat as otherwise
                            if otherwise_bb.is_none() {
                                otherwise_bb = Some(branch_bb);
                            }
                        }
                        Pattern::Member(type_pattern, variant_name) => {
                            // Member pattern for unit enum variants: Status.Ok
                            if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                                if let Some(crate::type_checker::context::TypeDefinition::Enum(
                                    enum_def,
                                )) = ctx.type_checker.global_type_definitions.get(type_name)
                                {
                                    if let Some((idx, _)) = enum_def
                                        .variants
                                        .iter()
                                        .enumerate()
                                        .find(|(_, (name, _))| *name == variant_name)
                                    {
                                        arm_discrs.push(idx as u128);
                                        if seen_discrs.insert(idx as u128) {
                                            switch_targets
                                                .push((Discriminant::from(idx as u128), branch_bb));
                                        }
                                    }
                                }
                            }
                        }
                        Pattern::EnumVariant(parent_pattern, _bindings) => {
                            // Enum variant with bindings: Color.Red(x, y)
                            if let Pattern::Member(type_pattern, variant_name) =
                                parent_pattern.as_ref()
                            {
                                if let Pattern::Identifier(type_name) = type_pattern.as_ref() {
                                    if let Some(
                                        crate::type_checker::context::TypeDefinition::Enum(
                                            enum_def,
                                        ),
                                    ) = ctx.type_checker.global_type_definitions.get(type_name)
                                    {
                                        if let Some((idx, _)) = enum_def
                                            .variants
                                            .iter()
                                            .enumerate()
                                            .find(|(_, (name, _))| *name == variant_name)
                                        {
                                            arm_discrs.push(idx as u128);
                                            if seen_discrs.insert(idx as u128) {
                                                switch_targets.push((
                                                    Discriminant::from(idx as u128),
                                                    branch_bb,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Pattern::Tuple(_) => {
                            // Tuple patterns match by structure - treat as otherwise for now
                            if otherwise_bb.is_none() {
                                otherwise_bb = Some(branch_bb);
                            }
                        }
                        Pattern::Regex(_) => {
                            // Regex patterns require runtime matching - treat as otherwise
                            if otherwise_bb.is_none() {
                                otherwise_bb = Some(branch_bb);
                            }
                        }
                    }
                }

                branch_blocks.push((branch_bb, branch, arm_discrs));
            }

            // Set otherwise to join if no default pattern
            let otherwise_target = otherwise_bb.unwrap_or(join_bb);

            // For enum types, we need to extract the discriminant (Field 0) to switch on
            let switch_discr = if let TypeKind::Custom(type_name, _) = &subject_ty.kind {
                if ctx
                    .type_checker
                    .global_type_definitions
                    .get(type_name)
                    .is_some_and(|td| {
                        matches!(td, crate::type_checker::context::TypeDefinition::Enum(_))
                    })
                {
                    // Extract discriminant from enum value at Field(0)
                    let discr_ty = Type::new(TypeKind::Int, subject.span);
                    let discr_local = ctx.push_temp(discr_ty, subject.span);

                    let mut discr_place = Place::new(subject_local);
                    discr_place.projection.push(PlaceElem::Field(0));

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(discr_local),
                            Rvalue::Use(Operand::Copy(discr_place)),
                        ),
                        span: subject.span,
                    });

                    Operand::Copy(Place::new(discr_local))
                } else {
                    Operand::Copy(Place::new(subject_local))
                }
            } else {
                Operand::Copy(Place::new(subject_local))
            };

            // Set SwitchInt terminator
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: switch_discr,
                    targets: switch_targets,
                    otherwise: otherwise_target,
                },
                expr.span,
            ));

            // Lower each branch body
            for (arm_idx, (branch_bb, branch, this_discrs)) in branch_blocks.iter().enumerate() {
                ctx.set_current_block(*branch_bb);
                ctx.push_scope();

                // Bind pattern variables
                for pattern in &branch.patterns {
                    bind_pattern(ctx, pattern, subject_local, &subject.span)?;
                }

                // Handle guard if present
                if let Some(guard) = &branch.guard {
                    let guard_op = lower_expression(ctx, guard, None)?;
                    let guard_true_bb = ctx.new_basic_block();

                    // Compute guard-failure target: the next arm that could match the same
                    // subject value.
                    //
                    // • If this arm has specific discriminants (literal / enum variant), scan
                    //   forward for the next arm that shares at least one of those discriminants,
                    //   OR the first catch-all arm (identifier / default / tuple / regex) —
                    //   whichever comes first in source order.
                    //
                    // • If this arm is itself a catch-all (empty discriminant set), scan forward
                    //   for the next catch-all arm.
                    //
                    // Falling off the end means no more arms can match → jump to join_bb.
                    let this_is_catchall = this_discrs.is_empty();
                    let mut guard_fail_bb = join_bb;
                    for (next_bb, _, next_discrs) in branch_blocks.iter().skip(arm_idx + 1) {
                        let next_is_catchall = next_discrs.is_empty();
                        if next_is_catchall {
                            // A catch-all arm can always receive control.
                            guard_fail_bb = *next_bb;
                            break;
                        }
                        if !this_is_catchall && this_discrs.iter().any(|d| next_discrs.contains(d))
                        {
                            // Next arm covers the same discriminant value.
                            guard_fail_bb = *next_bb;
                            break;
                        }
                    }

                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::SwitchInt {
                            discr: guard_op,
                            targets: vec![(Discriminant::bool_true(), guard_true_bb)],
                            otherwise: guard_fail_bb,
                        },
                        guard.span,
                    ));

                    ctx.set_current_block(guard_true_bb);
                }

                // Lower branch body and assign result to result_local
                lower_to_local(ctx, &branch.body, result_local, &result_ty)?;

                // Goto join if body didn't terminate (e.g., with return)
                if ctx.body.basic_blocks[ctx.current_block.0]
                    .terminator
                    .is_none()
                {
                    ctx.pop_scope(expr.span);
                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::Goto { target: join_bb },
                        expr.span,
                    ));
                }
            }

            ctx.set_current_block(join_bb);
            Ok(Operand::Copy(Place::new(result_local)))
        }
        ExpressionKind::Logical(lhs, op, rhs) => {
            // Short-circuit evaluation for logical operators:
            // - and: if lhs is false, skip rhs and return false
            // - or: if lhs is true, skip rhs and return true

            let result_ty = Type::new(TypeKind::Boolean, expr.span);
            let result_local = ctx.push_temp(result_ty.clone(), expr.span);

            // Evaluate LHS
            let lhs_op = lower_expression(ctx, lhs, None)?;

            // Create blocks for short-circuit evaluation
            let rhs_bb = ctx.new_basic_block();
            let done_bb = ctx.new_basic_block();

            match op {
                crate::ast::operator::BinaryOp::And => {
                    // and: if lhs is true, evaluate rhs; else return false
                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::SwitchInt {
                            discr: lhs_op.clone(),
                            targets: vec![(Discriminant::bool_true(), rhs_bb)], // true -> evaluate rhs
                            otherwise: done_bb, // false -> done with false
                        },
                        expr.span,
                    ));

                    // In done_bb after short-circuit (lhs was false), assign false
                    ctx.set_current_block(done_bb);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(result_local),
                            Rvalue::Use(Operand::Constant(Box::new(Constant {
                                span: expr.span,
                                ty: result_ty.clone(),
                                literal: crate::ast::literal::Literal::Boolean(false),
                            }))),
                        ),
                        span: expr.span,
                    });
                }
                crate::ast::operator::BinaryOp::Or => {
                    // or: if lhs is false, evaluate rhs; else return true
                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::SwitchInt {
                            discr: lhs_op.clone(),
                            targets: vec![(Discriminant::bool_false(), rhs_bb)], // false -> evaluate rhs
                            otherwise: done_bb, // true -> done with true
                        },
                        expr.span,
                    ));

                    // In done_bb after short-circuit (lhs was true), assign true
                    ctx.set_current_block(done_bb);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(result_local),
                            Rvalue::Use(Operand::Constant(Box::new(Constant {
                                span: expr.span,
                                ty: result_ty.clone(),
                                literal: crate::ast::literal::Literal::Boolean(true),
                            }))),
                        ),
                        span: expr.span,
                    });
                }
                _ => {
                    return Err(LoweringError::unsupported_operator(
                        format!("{:?}", op),
                        expr.span,
                    ));
                }
            }

            // Create final join block
            let final_bb = ctx.new_basic_block();
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: final_bb },
                expr.span,
            ));

            // Evaluate RHS in rhs_bb
            ctx.set_current_block(rhs_bb);
            let rhs_op = lower_expression(ctx, rhs, None)?;
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(rhs_op)),
                span: expr.span,
            });
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: final_bb },
                expr.span,
            ));

            ctx.set_current_block(final_bb);

            // DPS: if a destination was provided (e.g. the variable being initialised in
            // `let var = a and b`), write the result into it so the caller's variable is
            // populated.  Without this the Logical arm ignores `dest` and the variable
            // stays at its zero-initialised default (false).
            if let Some(ref d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        d.clone(),
                        Rvalue::Use(Operand::Copy(Place::new(result_local))),
                    ),
                    span: expr.span,
                });
                return Ok(Operand::Copy(d.clone()));
            }

            Ok(Operand::Copy(Place::new(result_local)))
        }
        ExpressionKind::Conditional(then_expr, cond_expr, else_expr_opt, if_type) => {
            // Inline if/unless expression: `value if condition else other`
            // then_expr is returned if condition is true (or false for unless)

            // Use dest if provided (DPS), otherwise create a temp
            let result_local = if let Some(ref dest_place) = dest {
                dest_place.local
            } else {
                let result_ty = resolve_type(ctx.type_checker, expr);
                ctx.push_temp(result_ty, expr.span)
            };

            // Evaluate condition first
            let cond_op = lower_expression(ctx, cond_expr, None)?;

            let then_bb = ctx.new_basic_block();
            let else_bb = ctx.new_basic_block();
            let join_bb = ctx.new_basic_block();

            // For `if`: true -> then, false -> else
            // For `unless`: true -> else, false -> then
            let (true_target, false_target) = match if_type {
                crate::ast::statement::IfStatementType::If => (then_bb, else_bb),
                crate::ast::statement::IfStatementType::Unless => (else_bb, then_bb),
            };

            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: cond_op,
                    targets: vec![(Discriminant::bool_true(), true_target)],
                    otherwise: false_target,
                },
                cond_expr.span,
            ));

            // Then block
            ctx.set_current_block(then_bb);
            let then_op = lower_expression(ctx, then_expr, None)?;
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(then_op)),
                span: then_expr.span,
            });
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: join_bb },
                then_expr.span,
            ));

            // Else block
            ctx.set_current_block(else_bb);
            if let Some(else_expr) = else_expr_opt {
                let else_op = lower_expression(ctx, else_expr, None)?;
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(else_op)),
                    span: else_expr.span,
                });
            }
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Goto { target: join_bb },
                expr.span,
            ));

            ctx.set_current_block(join_bb);
            Ok(Operand::Copy(Place::new(result_local)))
        }
        ExpressionKind::Range(start_expr, end_expr_opt, range_type) => {
            // Lower range expression to a tuple aggregate (start, end, is_inclusive)
            // This provides enough info for backends to iterate the range

            let start_op = lower_expression(ctx, start_expr, None)?;

            // End value - if not provided, create a "max" sentinel or just use start
            let end_op = if let Some(end_expr) = end_expr_opt {
                lower_expression(ctx, end_expr, None)?
            } else {
                // Range with no end (used for iterable objects) - use start as placeholder
                start_op.clone()
            };

            // is_inclusive flag
            let is_inclusive = matches!(
                range_type,
                crate::ast::expression::RangeExpressionType::Inclusive
            );
            let inclusive_op = Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: Type::new(TypeKind::Boolean, expr.span),
                literal: crate::ast::literal::Literal::Boolean(is_inclusive),
            }));

            // Create tuple aggregate (start, end, is_inclusive)
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, vec![start_op, end_op, inclusive_op]),
                ),
                span: expr.span,
            });
            Ok(Operand::Copy(Place::new(temp)))
        }
        ExpressionKind::Lambda(_generics, params, ret_type_expr, body, props) => {
            // Lambda expressions create an anonymous function.
            // We lower the body to a separate MIR Body and track captured variables.

            // Create a unique name for the lambda
            let lambda_id = expr.id;
            let lambda_name = format!("__lambda_{}", lambda_id);

            // Resolve the lambda's type (function type) from the type checker
            let lambda_ty = resolve_type(ctx.type_checker, expr);

            // Resolve return type
            let ret_ty = if let Some(ret_expr) = ret_type_expr {
                resolve_type(ctx.type_checker, ret_expr)
            } else {
                Type::new(TypeKind::Void, expr.span)
            };

            // Determine execution model
            let execution_model = if props.is_gpu {
                ExecutionModel::GpuKernel
            } else if props.is_async {
                ExecutionModel::Async
            } else {
                ExecutionModel::Cpu
            };

            // Create a new Body for the lambda
            let mut lambda_body = Body::new(params.len(), expr.span, execution_model);

            // _0: Return value
            lambda_body.new_local(LocalDecl::new(ret_ty.clone(), expr.span));

            // Create a nested context for the lambda
            // Note: We need to track which outer variables are captured
            let outer_variable_map = ctx.variable_map.clone();
            let mut lambda_ctx =
                LoweringContext::new(lambda_body, ctx.type_checker, ctx.is_release);

            // Add parameters to lambda context
            for param in params {
                let param_ty = resolve_type(ctx.type_checker, &param.typ);
                lambda_ctx.push_local(param.name.clone(), param_ty, param.typ.span);
            }

            // Lower the lambda body
            lower_as_return(&mut lambda_ctx, body, &ret_ty)?;

            // Ensure the last block has a terminator
            let last_block_idx = lambda_ctx.current_block.0;
            if lambda_ctx.body.basic_blocks[last_block_idx]
                .terminator
                .is_none()
            {
                lambda_ctx.set_terminator(Terminator::new(TerminatorKind::Return, expr.span));
            }

            // Detect captured variables: variables referenced in lambda that are
            // from the outer scope (not parameters)
            let mut captures: Vec<CapturedVar> = Vec::new();
            for (name, &outer_local) in &outer_variable_map {
                // Check if this outer variable was referenced in the lambda
                // by looking for it in the lambda's variable map
                if lambda_ctx.variable_map.contains_key(name) {
                    // If it's also a parameter, skip it (not a capture)
                    if params.iter().any(|p| p.name == name.as_ref()) {
                        continue;
                    }
                    // This is a captured variable - for now we just track it
                    // A more complete implementation would copy the value into the closure
                    if let Some(&lambda_local) = lambda_ctx.variable_map.get(name) {
                        captures.push(CapturedVar {
                            name: name.clone(),
                            lambda_local,
                            outer_local,
                        });
                    }
                }
            }

            // Store the lambda info
            let lambda_info = LambdaInfo {
                name: lambda_name.clone(),
                body: lambda_ctx.body,
                captures,
            };
            ctx.lambda_bodies.push(lambda_info);

            // Create a constant symbol representing the lambda
            // Backends will look up the lambda body by this name
            let temp = ctx.push_temp(lambda_ty.clone(), expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Use(Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: lambda_ty,
                        literal: crate::ast::literal::Literal::Symbol(lambda_name),
                    }))),
                ),
                span: expr.span,
            });

            Ok(Operand::Copy(Place::new(temp)))
        }
        ExpressionKind::FormattedString(parts) => {
            // Formatted string: f"Hello, {name}! Age: {age}"
            //
            // Each part is converted to a String via `emit_to_string` and then
            // all parts are concatenated left-to-right via String_concat.
            use crate::ast::literal::Literal;

            if parts.is_empty() {
                // Empty f-string: produce an empty string literal.
                let ty = Type::new(TypeKind::String, expr.span);
                let temp = ctx.push_temp(ty.clone(), expr.span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(temp),
                        Rvalue::Use(Operand::Constant(Box::new(Constant {
                            span: expr.span,
                            ty,
                            literal: Literal::String(String::new()),
                        }))),
                    ),
                    span: expr.span,
                });
                return Ok(Operand::Copy(Place::new(temp)));
            }

            // Convert each part to a String Local.
            let mut string_parts: Vec<crate::mir::place::Local> = Vec::with_capacity(parts.len());

            for part in parts.iter() {
                let part_op = lower_expression(ctx, part, None)?;

                // Determine the type of this part.
                let part_kind = ctx
                    .type_checker
                    .get_type(part.id)
                    .map(|t| t.kind.clone())
                    .unwrap_or_else(|| match &part_op {
                        Operand::Constant(c) => c.ty.kind.clone(),
                        Operand::Copy(p) | Operand::Move(p) => {
                            ctx.body.local_decls[p.local.0].ty.kind.clone()
                        }
                    });

                let string_local = emit_to_string(ctx, part_op, &part_kind, &expr.span)?;
                string_parts.push(string_local);
            }

            // Concatenate all parts left-to-right via String_concat.
            let mut accumulator = string_parts[0];
            for &next_part in &string_parts[1..] {
                let mut call_args = vec![
                    Operand::Copy(Place::new(accumulator)),
                    Operand::Copy(Place::new(next_part)),
                ];
                if let Some(&al) = ctx.variable_map.get("allocator") {
                    call_args.push(Operand::Copy(Place::new(al)));
                }
                let func_op = Operand::Constant(Box::new(Constant {
                    span: expr.span,
                    ty: Type::new(TypeKind::Symbol, expr.span),
                    literal: Literal::Symbol("String_concat".to_string()),
                }));
                let result = ctx.push_temp(Type::new(TypeKind::String, expr.span), expr.span);
                let target_bb = ctx.new_basic_block();
                ctx.set_terminator(Terminator::new(
                    TerminatorKind::Call {
                        func: func_op,
                        args: call_args,
                        destination: Place::new(result),
                        target: Some(target_bb),
                    },
                    expr.span,
                ));
                ctx.set_current_block(target_bb);
                accumulator = result;
            }

            Ok(Operand::Copy(Place::new(accumulator)))
        }
        ExpressionKind::Guard(guard_op, guard_expr) => {
            // Guard expressions are used in function parameter validation
            // e.g., fn divide(a int, b int > 0) - the `> 0` is a guard
            // We lower guards to comparison operations that return bool

            let operand = lower_expression(ctx, guard_expr, None)?;

            // Convert GuardOp to BinOp
            let _bin_op = match guard_op {
                crate::ast::operator::GuardOp::GreaterThan => BinOp::Gt,
                crate::ast::operator::GuardOp::GreaterThanEqual => BinOp::Ge,
                crate::ast::operator::GuardOp::LessThan => BinOp::Lt,
                crate::ast::operator::GuardOp::LessThanEqual => BinOp::Le,
                crate::ast::operator::GuardOp::NotEqual => BinOp::Ne,
                crate::ast::operator::GuardOp::Not => {
                    // Not is a unary op, apply directly
                    let result_ty = Type::new(TypeKind::Boolean, expr.span);
                    let temp = ctx.push_temp(result_ty, expr.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::UnaryOp(UnOp::Not, Box::new(operand)),
                        ),
                        span: expr.span,
                    });
                    return Ok(Operand::Copy(Place::new(temp)));
                }
                crate::ast::operator::GuardOp::In | crate::ast::operator::GuardOp::NotIn => {
                    // In/NotIn guards require membership test - for now create placeholder
                    return Ok(operand);
                }
            };

            // Guards already have their RHS value baked in from parsing
            // The operand IS the guard expression (e.g., the `0` in `> 0`)
            // The LHS (the parameter) would need to be provided by the caller
            // For now, just return the guard expression value
            Ok(operand)
        }
        ExpressionKind::NamedArgument(_name, value_expr) => {
            // Named argument: extract the value and lower it
            // The name is used by the type checker for struct field matching
            lower_expression(ctx, value_expr, None)
        }
        ExpressionKind::Super => {
            // Super refers to the parent class instance.
            // It's represented as a special constant that the backend
            // will use to resolve parent class method calls.
            // The type checker ensures this is only used in a derived class.
            let ty = resolve_type(ctx.type_checker, expr);
            let constant = Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty,
                literal: crate::ast::literal::Literal::Symbol("super".to_string()),
            }));

            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(constant.clone())),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                Ok(constant)
            }
        }
        ExpressionKind::EnumValue(enum_expr, args) => {
            // EnumValue is used for enum variant construction with :: syntax
            // e.g., Option::Some(value)
            // Extract the enum type name and variant from the expression
            if let ExpressionKind::Member(type_expr, variant_expr) = &enum_expr.node {
                if let ExpressionKind::Identifier(type_name, _) = &type_expr.node {
                    if let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node {
                        if let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                            ctx.type_checker.global_type_definitions.get(type_name)
                        {
                            if let Some((discriminant, _)) = enum_def
                                .variants
                                .iter()
                                .enumerate()
                                .find(|(_, (name, _))| name.as_str() == variant_name)
                            {
                                let ty = resolve_type(ctx.type_checker, expr);
                                let temp = ctx.push_temp(ty, expr.span);

                                // Create discriminant constant
                                let discr_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span,
                                    ty: Type::new(TypeKind::Int, expr.span),
                                    literal: crate::ast::literal::Literal::Integer(
                                        crate::ast::literal::IntegerLiteral::I32(
                                            discriminant as i32,
                                        ),
                                    ),
                                }));

                                // Lower all arguments
                                let mut ops = vec![discr_op];
                                for arg in args {
                                    ops.push(lower_expression(ctx, arg, None)?);
                                }

                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Aggregate(
                                            AggregateKind::Enum(
                                                type_name.clone(),
                                                variant_name.clone(),
                                            ),
                                            ops,
                                        ),
                                    ),
                                    span: expr.span,
                                });
                                return Ok(Operand::Copy(Place::new(temp)));
                            }
                        }
                    }
                }
            }
            Err(LoweringError::unsupported_expression(
                "Invalid EnumValue expression structure".to_string(),
                expr.span,
            ))
        }
        ExpressionKind::Type(ty, _is_nullable) => {
            // Type expressions used as values (metatype operations)
            // Return a symbol representing the type name
            let type_name = format!("{}", ty);
            let constant = Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: Type::new(TypeKind::Symbol, expr.span),
                literal: crate::ast::literal::Literal::Symbol(type_name),
            }));

            if let Some(d) = dest {
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(constant.clone())),
                    span: expr.span,
                });
                Ok(Operand::Copy(d))
            } else {
                Ok(constant)
            }
        }
        ExpressionKind::StructMember(_, _) => {
            // StructMember is primarily used in struct declarations, not runtime
            // If encountered at runtime, it's likely an error in the AST structure
            Err(LoweringError::unsupported_expression(
                "StructMember expressions are only valid in struct declarations".to_string(),
                expr.span,
            ))
        }
        ExpressionKind::GenericType(_, _, _) | ExpressionKind::TypeDeclaration(_, _, _, _) => {
            // Generic type instantiation and type declarations are compile-time only
            Err(LoweringError::unsupported_expression(
                "Type expressions are compile-time only, not runtime values".to_string(),
                expr.span,
            ))
        }
        ExpressionKind::ImportPath(_, _) => {
            // ImportPath should only appear in Use statements, not as standalone expressions
            Err(LoweringError::unsupported_expression(
                "ImportPath expressions are only valid in use statements".to_string(),
                expr.span,
            ))
        }
        ExpressionKind::Block(statements, final_expr) => {
            // Block expression: lower statements, then the final expression is the value
            for stmt in statements {
                lower_statement(ctx, stmt)?;
            }
            lower_expression(ctx, final_expr, dest)
        }
    }
}

/// Emits MIR to convert an operand to its String representation.
///
/// Handles `String` (identity), `Boolean` (cast to int → `miri_rt_bool_to_string`),
/// `Float`/`F64`/`F32` (promote to f64 → `miri_rt_float_to_string`), and all
/// integer types (`miri_rt_int_to_string`). Returns an error for unsupported types.
pub(super) fn emit_to_string(
    ctx: &mut LoweringContext,
    operand: Operand,
    type_kind: &TypeKind,
    span: &crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    match type_kind {
        TypeKind::String => {
            // Already a string — assign to a temp Local.
            let temp = ctx.push_temp(Type::new(TypeKind::String, *span), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(operand)),
                span: *span,
            });
            Ok(temp)
        }
        TypeKind::Boolean => {
            // Bool is I8 at the MIR level; widen to Int before calling runtime.
            let int_ty = Type::new(TypeKind::Int, *span);
            let int_temp = ctx.push_temp(int_ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(int_temp),
                    Rvalue::Cast(Box::new(operand), int_ty),
                ),
                span: *span,
            });
            let call_args = vec![Operand::Copy(Place::new(int_temp))];
            emit_runtime_to_string(ctx, "miri_rt_bool_to_string", call_args, span)
        }
        TypeKind::Float | TypeKind::F64 | TypeKind::F32 => {
            // miri_rt_float_to_string expects f64. Promote F32 if needed.
            let float_op = if matches!(type_kind, TypeKind::F32) {
                let f64_ty = Type::new(TypeKind::Float, *span);
                let f64_temp = ctx.push_temp(f64_ty.clone(), *span);
                ctx.push_statement(crate::mir::Statement {
                    kind: MirStatementKind::Assign(
                        Place::new(f64_temp),
                        Rvalue::Cast(Box::new(operand), f64_ty),
                    ),
                    span: *span,
                });
                Operand::Copy(Place::new(f64_temp))
            } else {
                operand
            };
            let call_args = vec![float_op];
            emit_runtime_to_string(ctx, "miri_rt_float_to_string", call_args, span)
        }
        TypeKind::Int | TypeKind::I64 | TypeKind::U64 => {
            // Already 64-bit — pass directly to the runtime (no widening needed).
            let call_args = vec![operand];
            emit_runtime_to_string(ctx, "miri_rt_int_to_string", call_args, span)
        }
        TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U128 => {
            // miri_rt_int_to_string expects i64. Widen the narrow integer to Int
            // (I64) before calling the runtime, mirroring the boolean path above.
            // Uses sextend (signed extension), which is correct for signed types
            // and for unsigned types with values ≤ the signed maximum of their width.
            let int_ty = Type::new(TypeKind::Int, *span);
            let int_temp = ctx.push_temp(int_ty.clone(), *span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(int_temp),
                    Rvalue::Cast(Box::new(operand), int_ty),
                ),
                span: *span,
            });
            let call_args = vec![Operand::Copy(Place::new(int_temp))];
            emit_runtime_to_string(ctx, "miri_rt_int_to_string", call_args, span)
        }
        other => Err(LoweringError::unsupported_expression(
            format!(
                "Cannot convert type '{}' to String in formatted string",
                other
            ),
            *span,
        )),
    }
}

/// Emits a call to a runtime type-to-string conversion function.
///
/// Creates a `TerminatorKind::Call` to the named runtime function, returns the
/// `Local` holding the resulting `String`.
fn emit_runtime_to_string(
    ctx: &mut LoweringContext,
    runtime_fn: &str,
    args: Vec<Operand>,
    span: &crate::error::syntax::Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    use crate::ast::literal::Literal;

    let result = ctx.push_temp(Type::new(TypeKind::String, *span), *span);
    let func_op = Operand::Constant(Box::new(Constant {
        span: *span,
        ty: Type::new(TypeKind::Symbol, *span),
        literal: Literal::Symbol(runtime_fn.to_string()),
    }));
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            destination: Place::new(result),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    Ok(result)
}
