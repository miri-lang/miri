// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    BinOp, Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind, UnOp,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

pub(crate) fn lower_binary_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Binary(lhs, op, rhs) = &expr.node else {
        unreachable!()
    };
    // Handle `In` operator specially - it's a membership test
    if matches!(op, crate::ast::operator::BinaryOp::In) {
        let lhs_op = lower_expression(ctx, lhs, None)?;
        let rhs_op = lower_expression(ctx, rhs, None)?;

        let result_ty = Type::new(TypeKind::Boolean, expr.span);
        let (destination, ret_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(result_ty, expr.span);
            (Place::new(temp), Operand::Copy(Place::new(temp)))
        };

        // Resolve the collection type to pick the right runtime function
        let fn_name = match ctx.type_checker.get_type(rhs.id).map(|t| &t.kind) {
            Some(TypeKind::Set(_)) => "miri_rt_set_contains",
            Some(TypeKind::Map(_, _)) => "miri_rt_map_contains_key",
            _ => "__contains",
        };

        let contains_fn = Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Identifier, expr.span),
            literal: crate::ast::literal::Literal::Identifier(fn_name.to_string()),
        }));

        let target_bb = ctx.new_basic_block();
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: contains_fn,
                args: vec![rhs_op, lhs_op], // (collection, element)
                destination,
                target: Some(target_bb),
            },
            expr.span,
        ));
        ctx.set_current_block(target_bb);

        return Ok(ret_op);
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
                crate::ast::operator::BinaryOp::Mul => Some(("Multiplicable", "repeat", false)),
                crate::ast::operator::BinaryOp::Equal => Some(("Equatable", "equals", false)),
                crate::ast::operator::BinaryOp::NotEqual => Some(("Equatable", "equals", true)),
                _ => None,
            };

            if let Some((_trait_name, method_name, negate)) = op_mapping {
                if let Some(crate::type_checker::context::TypeDefinition::Class(class_def)) =
                    ctx.type_checker.global_type_definitions.get(&class_name)
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
                            ty: Type::new(TypeKind::Identifier, expr.span),
                            literal: Literal::Identifier(mangled_name),
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
