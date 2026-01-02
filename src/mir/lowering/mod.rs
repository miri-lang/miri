// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

pub mod context;

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
        _props,
    ) = &ast_func.node
    {
        // For now, assume return type is Void if not specified, or handle it properly.
        // In a real compiler, we'd look up the type from the TypeChecker.
        let ret_ty = Type::new(TypeKind::Void, ast_func.span.clone());

        let mut body = Body::new(params.len(), ast_func.span.clone());

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

fn lower_statement(ctx: &mut LoweringContext, stmt: &Statement) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                lower_statement(ctx, s);
            }
        }
        StatementKind::Return(_) => {
            ctx.set_terminator(Terminator::new(TerminatorKind::Return, stmt.span.clone()));
        }
        StatementKind::Variable(decls, _) => {
            for decl in decls {
                let mut init_op = None;
                let var_ty;

                if let Some(init_expr) = &decl.initializer {
                    let op = lower_expression(ctx, init_expr);

                    // Try to get type from TypeChecker for the initializer expression
                    if let Some(ty) = ctx.type_checker.get_type(init_expr.id) {
                        var_ty = ty.clone();
                    } else {
                        // Fallback: infer from operand if constant or local
                        var_ty = match &op {
                            Operand::Constant(c) => c.ty.clone(),
                            Operand::Copy(place) | Operand::Move(place) => {
                                ctx.body.local_decls[place.local.0].ty.clone()
                            }
                        };
                    }
                    init_op = Some(op);
                } else if let Some(type_expr) = &decl.typ {
                    var_ty = resolve_type(ctx.type_checker, type_expr);
                } else {
                    panic!("Cannot determine type for variable '{}'", decl.name);
                }

                let local = ctx.push_local(decl.name.clone(), var_ty, stmt.span.clone());

                if let Some(op) = init_op {
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(op)),
                        span: stmt.span.clone(),
                    });
                }
            }
        }
        StatementKind::Expression(expr) => {
            let operand = lower_expression(ctx, expr);

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
        _ => {}
    }
}

fn lower_expression(ctx: &mut LoweringContext, expr: &Expression) -> Operand {
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
                panic!("Unknown variable: {}", name);
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
        _ => {
            panic!("Unsupported expression kind in lowering: {:?}", expr.node);
        }
    }
}

fn resolve_type(tc: &TypeChecker, expr: &Expression) -> Type {
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
