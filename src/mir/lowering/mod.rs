// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

pub mod context;
pub mod control_flow;
pub mod variable;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::mir::{
    BinOp, Body, Constant, LocalDecl, Operand, Place, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind, UnOp,
};
use crate::type_checker::TypeChecker;
use context::LoweringContext;

pub fn lower_function(ast_func: &Statement, tc: &TypeChecker) -> Result<Body, String> {
    if let StatementKind::FunctionDeclaration(
        _name,
        _generics,
        params,
        _ret_type,
        body_stmt,
        props,
    ) = &ast_func.node
    {
        // For now, assume return type is Void if not specified, or handle it properly.
        // In a real compiler, we'd look up the type from the TypeChecker.
        let ret_ty = Type::new(TypeKind::Void, ast_func.span.clone());

        let mut body = Body::new(params.len(), ast_func.span.clone(), props.is_gpu);

        // _0: Return value
        body.new_local(LocalDecl::new(ret_ty, ast_func.span.clone()));

        let mut ctx = LoweringContext::new(body, tc);

        // Add parameters as locals
        for param in params {
            // We need to resolve the type of the parameter.
            // For now, let's use a dummy type or try to extract it if simple.
            let param_ty = Type::new(TypeKind::Int, param.typ.span.clone()); // Placeholder
            ctx.push_local(param.name.clone(), param_ty, param.typ.span.clone());
        }

        // Lower body
        lower_statement(&mut ctx, body_stmt);

        // Ensure the last block has a terminator
        let last_block_idx = ctx.current_block.0;
        if ctx.body.basic_blocks[last_block_idx].terminator.is_none() {
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Return,
                ast_func.span.clone(),
            ));
        }

        Ok(ctx.body)
    } else {
        Err("Expected FunctionDeclaration".to_string())
    }
}

pub(crate) fn lower_statement(ctx: &mut LoweringContext, stmt: &Statement) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            // A block defines a new scope. Variables declared within
            // will be tracked and removed when the block ends.
            ctx.push_scope();
            for s in stmts {
                lower_statement(ctx, s);
            }
            ctx.pop_scope();
        }
        StatementKind::Return(_) => {
            ctx.set_terminator(Terminator::new(TerminatorKind::Return, stmt.span.clone()));
        }
        StatementKind::Variable(decls, _) => {
            variable::lower_variable(ctx, decls, &stmt.span);
        }
        StatementKind::Expression(expr) => {
            let operand = lower_expression(ctx, expr);

            // If the expression was a call (or other terminator-producing expr),
            // the current block might have changed or been terminated.
            // We only need to assign if it produced a value we care about,
            // but for expression statements, we usually discard the result unless it's a side-effect.
            // However, lower_expression typically returns an Operand which is valid in the
            // block active *after* the expression evaluation.

            // If the expression was a call, lower_expression emitted a call terminator
            // and switched to a new continuation block. The returned operand is a copy of the
            // destination temp in that new block.
            // We don't strictly *need* an assignment here if it's just an expression statement,
            // but for consistency with other expressions we can assign it to a temp.
            // The important part is that lower_expression handles the control flow.

            // Check if we need to emit an assignment.
            // If it's a call returning void, maybe we can skip assignment?
            // For now, keep generic behavior: assign to temp.

            let ty = match &operand {
                Operand::Constant(c) => c.ty.clone(),
                Operand::Copy(place) | Operand::Move(place) => {
                    ctx.body.local_decls[place.local.0].ty.clone()
                }
            };

            let temp = ctx.push_temp(ty, expr.span.clone());

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(operand)),
                span: expr.span.clone(),
            });
        }
        StatementKind::If(cond, then_block, else_block_opt, if_type) => {
            control_flow::lower_if(ctx, &stmt.span, cond, then_block, else_block_opt, if_type);
        }
        StatementKind::Break => {
            control_flow::lower_break(ctx, &stmt.span);
        }
        StatementKind::Continue => {
            control_flow::lower_continue(ctx, &stmt.span);
        }
        StatementKind::While(cond, body, while_type) => {
            control_flow::lower_while(ctx, &stmt.span, cond, body, while_type);
        }
        StatementKind::For(decls, iterable, body) => {
            control_flow::lower_for(ctx, &stmt.span, decls, iterable, body);
        }
        _ => {}
    }
}

pub(crate) fn lower_expression(ctx: &mut LoweringContext, expr: &Expression) -> Operand {
    match &expr.node {
        ExpressionKind::Literal(lit) => {
            let ty = match lit {
                crate::ast::literal::Literal::Integer(_) => {
                    Type::new(TypeKind::Int, expr.span.clone())
                }
                crate::ast::literal::Literal::Boolean(_) => {
                    Type::new(TypeKind::Boolean, expr.span.clone())
                }
                crate::ast::literal::Literal::String(_) => {
                    Type::new(TypeKind::String, expr.span.clone())
                }
                crate::ast::literal::Literal::Float(_) => {
                    Type::new(TypeKind::Float, expr.span.clone())
                }
                crate::ast::literal::Literal::Symbol(_) => {
                    Type::new(TypeKind::Symbol, expr.span.clone())
                }
                _ => Type::new(TypeKind::Void, expr.span.clone()),
            };

            Operand::Constant(Box::new(Constant {
                span: expr.span.clone(),
                ty,
                literal: lit.clone(),
            }))
        }
        ExpressionKind::Identifier(name, _) => {
            if let Some(local) = ctx.variable_map.get(name) {
                Operand::Copy(Place::new(*local))
            } else {
                // Assume global function/symbol
                // In a real compiler we would check if it exists in globals
                Operand::Constant(Box::new(Constant {
                    span: expr.span.clone(),
                    ty: Type::new(TypeKind::Symbol, expr.span.clone()),
                    literal: crate::ast::literal::Literal::Symbol(name.clone()),
                }))
            }
        }
        ExpressionKind::Assignment(lhs, op, rhs) => {
            match &**lhs {
                crate::ast::expression::LeftHandSideExpression::Identifier(id_expr) => {
                    if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                        let val = lower_expression(ctx, rhs);

                        if let Some(local) = ctx.variable_map.get(name) {
                            match op {
                                crate::ast::operator::AssignmentOp::Assign => {
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            Place::new(*local),
                                            Rvalue::Use(val.clone()),
                                        ),
                                        span: expr.span.clone(),
                                    });
                                }
                                _ => panic!("Unsupported assignment operator: {:?}", op),
                            }

                            // Assignment evaluates to the assigned value
                            val
                        } else {
                            panic!("Unknown variable in assignment: {}", name);
                        }
                    } else {
                        panic!("Expected identifier in assignment LHS");
                    }
                }
                _ => panic!("Unsupported LHS in assignment: {:?}", lhs),
            }
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            let lhs_op = lower_expression(ctx, lhs);
            let rhs_op = lower_expression(ctx, rhs);

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
                _ => panic!("Unsupported binary operator: {:?}", op),
            };

            let result_ty = match op {
                crate::ast::operator::BinaryOp::Equal
                | crate::ast::operator::BinaryOp::NotEqual
                | crate::ast::operator::BinaryOp::LessThan
                | crate::ast::operator::BinaryOp::LessThanEqual
                | crate::ast::operator::BinaryOp::GreaterThan
                | crate::ast::operator::BinaryOp::GreaterThanEqual => {
                    Type::new(TypeKind::Boolean, expr.span.clone())
                }
                _ => match &lhs_op {
                    Operand::Constant(c) => c.ty.clone(),
                    Operand::Copy(place) | Operand::Move(place) => {
                        ctx.body.local_decls[place.local.0].ty.clone()
                    }
                },
            };

            let temp = ctx.push_temp(result_ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(rhs_op)),
                ),
                span: expr.span.clone(),
            });

            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Unary(op, operand) => {
            let op_val = lower_expression(ctx, operand);
            let un_op = match op {
                crate::ast::operator::UnaryOp::Negate => UnOp::Neg,
                crate::ast::operator::UnaryOp::Not => UnOp::Not,
                _ => panic!("Unsupported unary operator: {:?}", op),
            };

            let result_ty = match &op_val {
                Operand::Constant(c) => c.ty.clone(),
                Operand::Copy(place) | Operand::Move(place) => {
                    ctx.body.local_decls[place.local.0].ty.clone()
                }
            };

            let temp = ctx.push_temp(result_ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::UnaryOp(un_op, Box::new(op_val)),
                ),
                span: expr.span.clone(),
            });

            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Call(func, args) => {
            // Check for GPU intrinsics
            if let ExpressionKind::Identifier(name, _) = &func.node {
                let intrinsic_rvalue = match name.as_str() {
                    "gpu_thread_idx_x" => Some(Rvalue::GpuThreadIdx(0)),
                    "gpu_thread_idx_y" => Some(Rvalue::GpuThreadIdx(1)),
                    "gpu_thread_idx_z" => Some(Rvalue::GpuThreadIdx(2)),
                    "gpu_block_idx_x" => Some(Rvalue::GpuBlockIdx(0)),
                    "gpu_block_idx_y" => Some(Rvalue::GpuBlockIdx(1)),
                    "gpu_block_idx_z" => Some(Rvalue::GpuBlockIdx(2)),
                    _ => None,
                };

                if let Some(rvalue) = intrinsic_rvalue {
                    let temp = ctx.push_temp(
                        Type::new(TypeKind::Int, expr.span.clone()),
                        expr.span.clone(),
                    );
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(Place::new(temp), rvalue),
                        span: expr.span.clone(),
                    });
                    return Operand::Copy(Place::new(temp));
                }
            }
            control_flow::lower_call(ctx, &expr.span, expr.id, func, args)
        }
        _ => {
            panic!("Unsupported expression kind in lowering: {:?}", expr.node);
        }
    }
}

pub(crate) fn resolve_type(tc: &TypeChecker, expr: &Expression) -> Type {
    if let Some(ty) = tc.get_type(expr.id) {
        return ty.clone();
    }

    match &expr.node {
        ExpressionKind::Type(t, is_nullable) => {
            if *is_nullable {
                Type::new(TypeKind::Nullable(t.clone()), expr.span.clone())
            } else {
                *t.clone()
            }
        }
        ExpressionKind::Identifier(name, _) => {
            if tc.global_type_definitions.contains_key(name) {
                Type::new(TypeKind::Custom(name.clone(), None), expr.span.clone())
            } else {
                match name.as_str() {
                    "int" => Type::new(TypeKind::Int, expr.span.clone()),
                    "bool" => Type::new(TypeKind::Boolean, expr.span.clone()),
                    "string" => Type::new(TypeKind::String, expr.span.clone()),
                    "float" => Type::new(TypeKind::Float, expr.span.clone()),
                    "void" => Type::new(TypeKind::Void, expr.span.clone()),
                    _ => panic!("Unknown type: {}", name),
                }
            }
        }
        _ => panic!("Unsupported type expression: {:?}", expr.node),
    }
}
