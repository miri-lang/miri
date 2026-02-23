// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering - converts AST to MIR (Mid-level Intermediate Representation).
//!
//! This module is organized into focused sub-modules:
//! - `context`: Lowering context and state management
//! - `control_flow`: Control flow constructs (if, while, for, break, continue)
//! - `expression`: Expression lowering (~1600 lines)
//! - `statement`: Statement lowering (~350 lines)
//! - `variable`: Variable declaration lowering
//! - `helpers`: Utility functions (resolve_type, bind_pattern, etc.)

pub mod context;
pub mod control_flow;
pub mod expression;
pub mod helpers;
pub mod statement;
pub mod variable;

use crate::ast::expression::ExpressionKind;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    BinOp, Body, Discriminant, ExecutionModel, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};
use crate::type_checker::TypeChecker;

// Re-export commonly used items from submodules
pub use context::LoweringContext;
pub use expression::lower_expression;
pub use helpers::{bind_pattern, literal_to_u128, lower_as_return, lower_to_local, resolve_type};
pub use statement::lower_statement;

/// Lower an AST function declaration to a MIR Body.
///
/// This is the main entry point for MIR lowering. It creates a lowering context,
/// processes parameters, emits guard checks, and lowers the function body.
pub fn lower_function(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
) -> Result<Body, LoweringError> {
    if let StatementKind::FunctionDeclaration(
        _name,
        _generics,
        params,
        ret_type_expr,
        body_stmt,
        props,
    ) = &ast_func.node
    {
        // Resolve return type from the function signature
        let name_str = if let StatementKind::FunctionDeclaration(n, ..) = &ast_func.node {
            n
        } else {
            ""
        };

        let ret_ty = if let Some(ret_expr) = ret_type_expr {
            resolve_type(tc, ret_expr)
        } else if let Some(ty) = tc.get_variable_type(name_str) {
            if let TypeKind::Function(_, _, Some(rt)) = &ty.kind {
                resolve_type(tc, rt)
            } else {
                Type::new(TypeKind::Void, ast_func.span.clone())
            }
        } else {
            Type::new(TypeKind::Void, ast_func.span.clone())
        };

        let execution_model = if props.is_gpu {
            ExecutionModel::GpuKernel
        } else if props.is_async {
            ExecutionModel::Async
        } else {
            ExecutionModel::Cpu
        };

        // Initialize lowering context
        let body = Body::new(params.len(), ast_func.span.clone(), execution_model);
        let mut ctx = LoweringContext::new(body, tc, is_release);

        // _0: Return value
        ctx.body
            .new_local(LocalDecl::new(ret_ty.clone(), ast_func.span.clone()));

        // Lower parameters
        for param in params.iter() {
            let param_ty = resolve_type(tc, &param.typ);
            ctx.push_param(param.name.clone(), param_ty, param.typ.span.clone());
        }

        // Implicit Allocator Injection
        // We inject an 'allocator' parameter to specific functions (or all for now)
        // This supports the "Call Site Allocator Injection" strategy.
        if inject_allocator {
            let allocator_type = Type::new(TypeKind::Int, ast_func.span.clone());

            if name_str == "main" {
                // For main, we cannot inject a parameter as it breaks the entry point signature.
                // Instead, we create a local variable 'allocator'.
                // Ideally this should be initialized by the runtime, but for now we leave it uninitialized
                // (StorageLive only) which is sufficient if it's just passed around and not dereferenced.
                let alloc_local = ctx.push_local(
                    "allocator".to_string(),
                    allocator_type.clone(),
                    ast_func.span.clone(),
                );

                // Initialize to dummy value (0) to avoid uninitialized read
                let dummy_allocator =
                    crate::mir::Operand::Constant(Box::new(crate::mir::Constant {
                        span: ast_func.span.clone(),
                        ty: allocator_type,
                        literal: crate::ast::literal::Literal::Integer(
                            crate::ast::literal::IntegerLiteral::I32(0),
                        ),
                    }));

                ctx.push_statement(crate::mir::Statement {
                    kind: crate::mir::StatementKind::Assign(
                        crate::mir::Place::new(alloc_local),
                        crate::mir::Rvalue::Use(dummy_allocator),
                    ),
                    span: ast_func.span.clone(),
                });
            } else {
                ctx.push_param(
                    "allocator".to_string(),
                    allocator_type,
                    ast_func.span.clone(),
                );
                ctx.body.arg_count += 1;
            }
        }

        // Emit guard checks for parameters with guards
        for param in params {
            if let Some(guard) = &param.guard {
                if let Some(&param_local) = ctx.variable_map.get(param.name.as_str()) {
                    if let ExpressionKind::Guard(guard_op, guard_value) = &guard.node {
                        let guard_val = lower_expression(&mut ctx, guard_value, None)?;

                        let bin_op = match guard_op {
                            crate::ast::operator::GuardOp::GreaterThan => BinOp::Gt,
                            crate::ast::operator::GuardOp::GreaterThanEqual => BinOp::Ge,
                            crate::ast::operator::GuardOp::LessThan => BinOp::Lt,
                            crate::ast::operator::GuardOp::LessThanEqual => BinOp::Le,
                            crate::ast::operator::GuardOp::NotEqual => BinOp::Ne,
                            _ => continue,
                        };

                        let check_result = ctx.push_temp(
                            Type::new(TypeKind::Boolean, guard.span.clone()),
                            guard.span.clone(),
                        );
                        ctx.push_statement(crate::mir::Statement {
                            kind: MirStatementKind::Assign(
                                Place::new(check_result),
                                Rvalue::BinaryOp(
                                    bin_op,
                                    Box::new(Operand::Copy(Place::new(param_local))),
                                    Box::new(guard_val),
                                ),
                            ),
                            span: guard.span.clone(),
                        });

                        let continue_bb = ctx.new_basic_block();
                        let fail_bb = ctx.new_basic_block();

                        ctx.set_terminator(Terminator::new(
                            TerminatorKind::SwitchInt {
                                discr: Operand::Copy(Place::new(check_result)),
                                targets: vec![(Discriminant::bool_true(), continue_bb)],
                                otherwise: fail_bb,
                            },
                            guard.span.clone(),
                        ));

                        ctx.set_current_block(fail_bb);
                        ctx.set_terminator(Terminator::new(
                            TerminatorKind::Unreachable,
                            guard.span.clone(),
                        ));

                        ctx.set_current_block(continue_bb);
                    }
                }
            }
        }

        // Lower body with support for implicit return
        if let Some(body_box) = body_stmt {
            lower_as_return(&mut ctx, body_box, &ret_ty)?;
        }

        // Pop root scope variables if falling through
        if ctx.body.basic_blocks[ctx.current_block.0]
            .terminator
            .is_none()
        {
            ctx.pop_scope(ast_func.span.clone());
        }

        // Ensure the last block has a terminator
        let last_block_idx = ctx.current_block.0;
        if ctx.body.basic_blocks[last_block_idx].terminator.is_none() {
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Return,
                ast_func.span.clone(),
            ));
        }

        // Validate the body
        if let Err(msg) = ctx.body.validate() {
            return Err(LoweringError::custom(
                format!("MIR Validation Error: {}", msg),
                ast_func.span.clone(),
                None,
            ));
        }

        Ok(ctx.body)
    } else {
        Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration".to_string(),
            ast_func.span.clone(),
        ))
    }
}

/// Lower a stdlib class method to a MIR Body.
///
/// Unlike `lower_function`, this variant:
/// - Prepends an implicit `self` parameter (registered in `variable_map`)
/// - Registers the allocator in the function ABI (`body.arg_count`) but NOT in
///   the lowering context's `variable_map`.
///
/// Keeping the allocator out of `variable_map` prevents the auto-injector from
/// appending it to calls to runtime C functions inside the method body. Those C
/// functions do not accept an allocator parameter.
pub fn lower_class_method(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
) -> Result<Body, LoweringError> {
    if let StatementKind::FunctionDeclaration(
        _name,
        _generics,
        params,
        ret_type_expr,
        body_stmt,
        props,
    ) = &ast_method.node
    {
        let ret_ty = if let Some(ret_expr) = ret_type_expr {
            resolve_type(tc, ret_expr)
        } else {
            Type::new(TypeKind::Void, ast_method.span.clone())
        };

        let execution_model = if props.is_gpu {
            ExecutionModel::GpuKernel
        } else if props.is_async {
            ExecutionModel::Async
        } else {
            ExecutionModel::Cpu
        };

        // Initial arg_count = 1 (self) + explicit params.
        // The allocator is added to arg_count below but NOT to variable_map.
        let body = Body::new(params.len() + 1, ast_method.span.clone(), execution_model);
        let mut ctx = LoweringContext::new(body, tc, is_release);

        // _0: Return value
        ctx.body
            .new_local(LocalDecl::new(ret_ty.clone(), ast_method.span.clone()));

        // _1: self parameter (the class instance, registered in variable_map)
        ctx.push_param("self".to_string(), self_type, ast_method.span.clone());

        // Remaining explicit parameters (registered in variable_map)
        for param in params.iter() {
            let param_ty = resolve_type(tc, &param.typ);
            ctx.push_param(param.name.clone(), param_ty, param.typ.span.clone());
        }

        // Inject allocator into the ABI for call-site compatibility, but do NOT
        // register it in variable_map so that internal calls to runtime C functions
        // do not receive the allocator as an extra argument.
        {
            let allocator_decl = LocalDecl::new(
                Type::new(TypeKind::Int, ast_method.span.clone()),
                ast_method.span.clone(),
            );
            ctx.body.new_local(allocator_decl);
            ctx.body.arg_count += 1;
        }

        // Lower body
        if let Some(body_box) = body_stmt {
            lower_as_return(&mut ctx, body_box, &ret_ty)?;
        }

        // Pop root scope if falling through
        if ctx.body.basic_blocks[ctx.current_block.0]
            .terminator
            .is_none()
        {
            ctx.pop_scope(ast_method.span.clone());
        }

        // Ensure last block has a terminator
        let last_block_idx = ctx.current_block.0;
        if ctx.body.basic_blocks[last_block_idx].terminator.is_none() {
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Return,
                ast_method.span.clone(),
            ));
        }

        if let Err(msg) = ctx.body.validate() {
            return Err(LoweringError::custom(
                format!("MIR Validation Error: {}", msg),
                ast_method.span.clone(),
                None,
            ));
        }

        Ok(ctx.body)
    } else {
        Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration for class method".to_string(),
            ast_method.span.clone(),
        ))
    }
}
