// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

pub mod context;
pub mod control_flow;
pub mod variable;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::Pattern;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::mir::{
    AggregateKind, BinOp, Body, Constant, Dimension, ExecutionModel, GpuIntrinsic, LocalDecl,
    Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind, UnOp,
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
        // TODO: Return type is currently assumed to be Void. Should look up the
        // actual return type from the function signature via the TypeChecker.
        let ret_ty = Type::new(TypeKind::Void, ast_func.span.clone());

        let execution_model = if props.is_gpu {
            ExecutionModel::GpuKernel
        } else {
            ExecutionModel::Cpu
        };
        let mut body = Body::new(params.len(), ast_func.span.clone(), execution_model);

        // _0: Return value
        body.new_local(LocalDecl::new(ret_ty, ast_func.span.clone()));

        let mut ctx = LoweringContext::new(body, tc);

        for param in params {
            // TODO: Parameter type resolution is currently a placeholder. Should resolve
            // the actual type from param.typ expression via the TypeChecker.
            let param_ty = Type::new(TypeKind::Int, param.typ.span.clone());
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
                                // Variant with associated values
                                let ty = resolve_type(ctx.type_checker, expr);
                                let temp = ctx.push_temp(ty, expr.span.clone());

                                // Create discriminant constant
                                let discr_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span.clone(),
                                    ty: Type::new(TypeKind::Int, expr.span.clone()),
                                    literal: crate::ast::literal::Literal::Integer(
                                        crate::ast::literal::IntegerLiteral::I32(
                                            discriminant as i32,
                                        ),
                                    ),
                                }));

                                // Lower all arguments
                                let mut ops = vec![discr_op];
                                for arg in args {
                                    ops.push(lower_expression(ctx, arg));
                                }

                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Aggregate(AggregateKind::Tuple, ops),
                                    ),
                                    span: expr.span.clone(),
                                });
                                return Operand::Copy(Place::new(temp));
                            }
                        }
                    }
                }
            }

            control_flow::lower_call(ctx, &expr.span, expr.id, func, args)
        }
        ExpressionKind::Member(obj, prop) => {
            let obj_operand = lower_expression(ctx, obj);

            // Handle GPU Intrinsics (gpu_context.thread_idx.x, etc.)
            // This uses a two-step lowering: gpu_context.thread_idx => intermediate symbol,
            // then intermediate_symbol.x => actual GpuIntrinsic rvalue.
            if let Operand::Constant(c) = &obj_operand {
                if let crate::ast::literal::Literal::Symbol(sym) = &c.literal {
                    if sym == "gpu_context" {
                        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                            // Return intermediate symbol for chained access
                            return Operand::Constant(Box::new(Constant {
                                span: expr.span.clone(),
                                ty: Type::new(TypeKind::Void, expr.span.clone()),
                                literal: crate::ast::literal::Literal::Symbol(format!(
                                    "gpu_context.{}",
                                    prop_name
                                )),
                            }));
                        }
                    } else if sym.starts_with("gpu_context.") {
                        if let ExpressionKind::Identifier(prop_name, _) = &prop.node {
                            let dim = match prop_name.as_str() {
                                "x" => Dimension::X,
                                "y" => Dimension::Y,
                                "z" => Dimension::Z,
                                _ => panic!("Invalid dimension for GPU intrinsic: {}", prop_name),
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
                                _ => panic!("Unknown GPU intrinsic: {}", sym),
                            };

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
                }
            }

            // 2. Handle General Struct Member Access
            let obj_ty = if let Some(ty) = ctx.type_checker.get_type(obj.id) {
                ty
            } else {
                panic!("Type not found for expression ID {}", obj.id);
            };

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
                                    let temp = ctx.push_temp(c.ty.clone(), obj.span.clone());
                                    ctx.push_statement(crate::mir::Statement {
                                        kind: MirStatementKind::Assign(
                                            Place::new(temp),
                                            Rvalue::Use(Operand::Constant(c)),
                                        ),
                                        span: obj.span.clone(),
                                    });
                                    Place::new(temp)
                                }
                            };

                            // Create new place with projection
                            let mut new_place = place.clone();
                            new_place.projection.push(PlaceElem::Field(idx));

                            return Operand::Copy(new_place);
                        } else {
                            panic!(
                                "Field '{}' not found in struct '{}'",
                                field_name, struct_name
                            );
                        }
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
                            let associated_types = enum_def.variants.get(variant_name).unwrap();
                            if associated_types.is_empty() {
                                let ty = resolve_type(ctx.type_checker, expr);
                                let temp = ctx.push_temp(ty, expr.span.clone());

                                // Create discriminant constant
                                let discr_op = Operand::Constant(Box::new(Constant {
                                    span: expr.span.clone(),
                                    ty: Type::new(TypeKind::Int, expr.span.clone()),
                                    literal: crate::ast::literal::Literal::Integer(
                                        crate::ast::literal::IntegerLiteral::I32(
                                            discriminant as i32,
                                        ),
                                    ),
                                }));

                                ctx.push_statement(crate::mir::Statement {
                                    kind: MirStatementKind::Assign(
                                        Place::new(temp),
                                        Rvalue::Aggregate(AggregateKind::Tuple, vec![discr_op]),
                                    ),
                                    span: expr.span.clone(),
                                });
                                return Operand::Copy(Place::new(temp));
                            }
                            // Variant with associated values - handled in Call
                        }
                    }
                }
            }

            panic!("Unsupported member access on type: {}", obj_ty);
        }
        ExpressionKind::Tuple(elements) => {
            let ops: Vec<Operand> = elements.iter().map(|e| lower_expression(ctx, e)).collect();
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Tuple, ops),
                ),
                span: expr.span.clone(),
            });
            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::List(elements) => {
            let ops: Vec<Operand> = elements.iter().map(|e| lower_expression(ctx, e)).collect();
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::List, ops),
                ),
                span: expr.span.clone(),
            });
            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Array(elements, _size) => {
            let ops: Vec<Operand> = elements.iter().map(|e| lower_expression(ctx, e)).collect();
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Array, ops),
                ),
                span: expr.span.clone(),
            });
            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Set(elements) => {
            let ops: Vec<Operand> = elements.iter().map(|e| lower_expression(ctx, e)).collect();
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Set, ops),
                ),
                span: expr.span.clone(),
            });
            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Map(pairs) => {
            // Flatten pairs into [key1, val1, key2, val2, ...]
            let mut ops: Vec<Operand> = Vec::with_capacity(pairs.len() * 2);
            for (key, val) in pairs {
                ops.push(lower_expression(ctx, key));
                ops.push(lower_expression(ctx, val));
            }
            let ty = resolve_type(ctx.type_checker, expr);
            let temp = ctx.push_temp(ty, expr.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::Aggregate(AggregateKind::Map, ops),
                ),
                span: expr.span.clone(),
            });
            Operand::Copy(Place::new(temp))
        }
        ExpressionKind::Index(obj, index_expr) => {
            // Lower object to get a place
            let obj_operand = lower_expression(ctx, obj);

            // Ensure object is in a place (copy to temp if constant)
            let obj_place = match obj_operand {
                Operand::Copy(p) | Operand::Move(p) => p,
                Operand::Constant(c) => {
                    let temp = ctx.push_temp(c.ty.clone(), obj.span.clone());
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Use(Operand::Constant(c)),
                        ),
                        span: obj.span.clone(),
                    });
                    Place::new(temp)
                }
            };

            // Lower index expression
            let index_operand = lower_expression(ctx, index_expr);

            // Ensure index is in a local (PlaceElem::Index requires Local)
            let index_local = match index_operand {
                Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
                _ => {
                    // Store in temp - use inferred type from type checker or default to Int
                    let ty = ctx
                        .type_checker
                        .get_type(index_expr.id)
                        .cloned()
                        .unwrap_or_else(|| Type::new(TypeKind::Int, index_expr.span.clone()));
                    let temp = ctx.push_temp(ty, index_expr.span.clone());
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(temp),
                            Rvalue::Use(index_operand),
                        ),
                        span: index_expr.span.clone(),
                    });
                    temp
                }
            };

            // Create indexed place with projection
            let mut indexed_place = obj_place;
            indexed_place.projection.push(PlaceElem::Index(index_local));

            Operand::Copy(indexed_place)
        }
        ExpressionKind::Match(subject, branches) => {
            // Lower the subject expression
            let subject_op = lower_expression(ctx, subject);

            // Store subject in a temp so we can reference it multiple times
            let subject_ty = resolve_type(ctx.type_checker, subject);
            let subject_local = ctx.push_temp(subject_ty.clone(), subject.span.clone());
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(subject_local), Rvalue::Use(subject_op)),
                span: subject.span.clone(),
            });

            // Result type and temp
            let result_ty = resolve_type(ctx.type_checker, expr);
            let result_local = ctx.push_temp(result_ty.clone(), expr.span.clone());

            // Create join block where all branches converge
            let join_bb = ctx.new_basic_block();

            // Collect literal patterns for SwitchInt
            let mut switch_targets: Vec<(u128, crate::mir::block::BasicBlock)> = Vec::new();
            let mut otherwise_bb = None;
            let mut branch_blocks: Vec<(
                crate::mir::block::BasicBlock,
                &crate::ast::pattern::MatchBranch,
            )> = Vec::new();

            for branch in branches {
                let branch_bb = ctx.new_basic_block();
                branch_blocks.push((branch_bb, branch));

                for pattern in &branch.patterns {
                    match pattern {
                        Pattern::Literal(lit) => {
                            if let Some(val) = literal_to_u128(lit) {
                                switch_targets.push((val, branch_bb));
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
                        _ => {
                            // Tuple, Regex, Member patterns - for now treat as otherwise
                            if otherwise_bb.is_none() {
                                otherwise_bb = Some(branch_bb);
                            }
                        }
                    }
                }
            }

            // Set otherwise to join if no default pattern
            let otherwise_target = otherwise_bb.unwrap_or(join_bb);

            // Set SwitchInt terminator
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::new(subject_local)),
                    targets: switch_targets,
                    otherwise: otherwise_target,
                },
                expr.span.clone(),
            ));

            // Lower each branch body
            for (branch_bb, branch) in branch_blocks {
                ctx.set_current_block(branch_bb);

                // Bind pattern variables
                for pattern in &branch.patterns {
                    bind_pattern(ctx, pattern, subject_local, &subject.span);
                }

                // Handle guard if present
                if let Some(guard) = &branch.guard {
                    let guard_op = lower_expression(ctx, guard);
                    let guard_true_bb = ctx.new_basic_block();

                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::SwitchInt {
                            discr: guard_op,
                            targets: vec![(1, guard_true_bb)],
                            otherwise: otherwise_target,
                        },
                        guard.span.clone(),
                    ));

                    ctx.set_current_block(guard_true_bb);
                }

                // Lower branch body
                lower_statement(ctx, &branch.body);

                // Assign result (if body is expression statement)
                // For now, just goto join
                if ctx.body.basic_blocks[ctx.current_block.0]
                    .terminator
                    .is_none()
                {
                    ctx.set_terminator(Terminator::new(
                        TerminatorKind::Goto { target: join_bb },
                        expr.span.clone(),
                    ));
                }
            }

            ctx.set_current_block(join_bb);
            Operand::Copy(Place::new(result_local))
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

/// Convert a literal to u128 for SwitchInt discrimination.
fn literal_to_u128(lit: &crate::ast::literal::Literal) -> Option<u128> {
    use crate::ast::literal::{IntegerLiteral, Literal};
    match lit {
        Literal::Integer(int_lit) => match int_lit {
            IntegerLiteral::I8(v) => Some(*v as u128),
            IntegerLiteral::I16(v) => Some(*v as u128),
            IntegerLiteral::I32(v) => Some(*v as u128),
            IntegerLiteral::I64(v) => Some(*v as u128),
            IntegerLiteral::I128(v) => Some(*v as u128),
            IntegerLiteral::U8(v) => Some(*v as u128),
            IntegerLiteral::U16(v) => Some(*v as u128),
            IntegerLiteral::U32(v) => Some(*v as u128),
            IntegerLiteral::U64(v) => Some(*v as u128),
            IntegerLiteral::U128(v) => Some(*v),
        },
        Literal::Boolean(b) => Some(if *b { 1 } else { 0 }),
        // String, Float, Symbol - can't be used with SwitchInt directly
        _ => None,
    }
}

/// Bind pattern variables to the subject value.
fn bind_pattern(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject_local: crate::mir::Local,
    span: &crate::error::syntax::Span,
) {
    match pattern {
        Pattern::Identifier(name) => {
            // Create a new local for the bound variable
            let ty = ctx.body.local_decls[subject_local.0].ty.clone();
            let var_local = ctx.push_temp(ty, span.clone());
            ctx.body.local_decls[var_local.0].name = Some(name.clone());

            // Assign subject value to bound variable
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(var_local),
                    Rvalue::Use(Operand::Copy(Place::new(subject_local))),
                ),
                span: span.clone(),
            });

            // Register in variable map
            ctx.variable_map.insert(name.clone(), var_local);
        }
        Pattern::Tuple(patterns) => {
            // For tuple destructuring, create bindings for each element
            for (i, p) in patterns.iter().enumerate() {
                if let Pattern::Identifier(name) = p {
                    let ty = ctx.body.local_decls[subject_local.0].ty.clone();
                    let elem_local = ctx.push_temp(ty, span.clone());
                    ctx.body.local_decls[elem_local.0].name = Some(name.clone());

                    // Create indexed place for tuple element
                    let mut place = Place::new(subject_local);
                    let idx_local =
                        ctx.push_temp(Type::new(TypeKind::Int, span.clone()), span.clone());
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(idx_local),
                            Rvalue::Use(Operand::Constant(Box::new(Constant {
                                span: span.clone(),
                                ty: Type::new(TypeKind::Int, span.clone()),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I32(i as i32),
                                ),
                            }))),
                        ),
                        span: span.clone(),
                    });
                    place.projection.push(PlaceElem::Index(idx_local));

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(elem_local),
                            Rvalue::Use(Operand::Copy(place)),
                        ),
                        span: span.clone(),
                    });

                    ctx.variable_map.insert(name.clone(), elem_local);
                }
            }
        }
        Pattern::EnumVariant(_parent, bindings) => {
            // For enum variant destructuring, extract associated values
            // The aggregate is (discriminant, val1, val2, ...), so bindings start at index 1
            for (i, binding) in bindings.iter().enumerate() {
                if let Pattern::Identifier(name) = binding {
                    // Create local for bound variable
                    // Note: Type info is from type checker, we use a generic type here
                    let ty = Type::new(TypeKind::Void, span.clone()); // Will be properly typed
                    let elem_local = ctx.push_temp(ty, span.clone());
                    ctx.body.local_decls[elem_local.0].name = Some(name.clone());

                    // Create indexed place for element (index i+1 to skip discriminant)
                    let mut place = Place::new(subject_local);
                    let idx_local =
                        ctx.push_temp(Type::new(TypeKind::Int, span.clone()), span.clone());
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(idx_local),
                            Rvalue::Use(Operand::Constant(Box::new(Constant {
                                span: span.clone(),
                                ty: Type::new(TypeKind::Int, span.clone()),
                                literal: crate::ast::literal::Literal::Integer(
                                    crate::ast::literal::IntegerLiteral::I32((i + 1) as i32), // Skip discriminant at index 0
                                ),
                            }))),
                        ),
                        span: span.clone(),
                    });
                    place.projection.push(PlaceElem::Index(idx_local));

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(elem_local),
                            Rvalue::Use(Operand::Copy(place)),
                        ),
                        span: span.clone(),
                    });

                    ctx.variable_map.insert(name.clone(), elem_local);
                }
            }
        }
        // Literal, Default, Regex, Member - no bindings needed
        _ => {}
    }
}
