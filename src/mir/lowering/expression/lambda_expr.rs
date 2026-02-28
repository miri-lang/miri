// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::lambda::{CapturedVar, LambdaInfo};
use crate::mir::{
    Body, Constant, ExecutionModel, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::{lower_as_return, resolve_type};

pub(crate) fn lower_lambda_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    _dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Lambda(lambda) = &expr.node else {
        unreachable!()
    };
    let params = &lambda.params;
    let ret_type_expr = &lambda.return_type;
    let body = &lambda.body;
    let props = &lambda.properties;
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
    let mut lambda_ctx = LoweringContext::new(lambda_body, ctx.type_checker, ctx.is_release);

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
