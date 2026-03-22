// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::statement::lower_statement;

pub mod array_expr;
pub mod assignment_expr;
pub mod binary_expr;
pub mod call_expr;
pub mod conditional_expr;
pub mod enumvalue_expr;
pub mod formattedstring_expr;
pub mod guard_expr;
pub mod identifier_expr;
pub mod index_expr;
pub mod lambda_expr;
pub mod list_expr;
pub mod literal_expr;
pub mod logical_expr;
pub mod map_expr;
pub mod match_expr;
pub mod member_expr;
pub mod namedargument_expr;
pub mod range_expr;
pub mod set_expr;

pub mod structmember_expr;
pub mod super_expr;
pub mod tuple_expr;
pub mod type_expr;
pub mod unary_expr;

pub fn lower_expression(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    match &expr.node {
        ExpressionKind::Literal(..) => literal_expr::lower_literal_expr(ctx, expr, dest),
        ExpressionKind::Identifier(..) => identifier_expr::lower_identifier_expr(ctx, expr, dest),
        ExpressionKind::Assignment(..) => assignment_expr::lower_assignment_expr(ctx, expr, dest),
        ExpressionKind::Binary(..) => binary_expr::lower_binary_expr(ctx, expr, dest),
        ExpressionKind::Unary(..) => unary_expr::lower_unary_expr(ctx, expr, dest),
        ExpressionKind::Call(..) => call_expr::lower_call_expr(ctx, expr, dest),
        ExpressionKind::Member(..) => member_expr::lower_member_expr(ctx, expr, dest),
        ExpressionKind::Tuple(..) => tuple_expr::lower_tuple_expr(ctx, expr, dest),
        ExpressionKind::List(..) => list_expr::lower_list_expr(ctx, expr, dest),
        ExpressionKind::Array(..) => array_expr::lower_array_expr(ctx, expr, dest),

        ExpressionKind::Set(..) => set_expr::lower_set_expr(ctx, expr, dest),
        ExpressionKind::Map(..) => map_expr::lower_map_expr(ctx, expr, dest),
        ExpressionKind::Index(..) => index_expr::lower_index_expr(ctx, expr, dest),
        ExpressionKind::Match(..) => match_expr::lower_match_expr(ctx, expr, dest),
        ExpressionKind::Logical(..) => logical_expr::lower_logical_expr(ctx, expr, dest),
        ExpressionKind::Conditional(..) => {
            conditional_expr::lower_conditional_expr(ctx, expr, dest)
        }
        ExpressionKind::Range(..) => range_expr::lower_range_expr(ctx, expr, dest),
        ExpressionKind::Lambda(..) => lambda_expr::lower_lambda_expr(ctx, expr, dest),
        ExpressionKind::FormattedString(..) => {
            formattedstring_expr::lower_formattedstring_expr(ctx, expr, dest)
        }
        ExpressionKind::Guard(..) => guard_expr::lower_guard_expr(ctx, expr, dest),
        ExpressionKind::NamedArgument(..) => {
            namedargument_expr::lower_namedargument_expr(ctx, expr, dest)
        }
        ExpressionKind::Super => super_expr::lower_super_expr(ctx, expr, dest),
        ExpressionKind::EnumValue(..) => enumvalue_expr::lower_enumvalue_expr(ctx, expr, dest),
        ExpressionKind::Type(..) => type_expr::lower_type_expr(ctx, expr, dest),
        ExpressionKind::StructMember(..) => {
            structmember_expr::lower_structmember_expr(ctx, expr, dest)
        }
        ExpressionKind::GenericType(_, _, _) | ExpressionKind::TypeDeclaration(_, _, _, _) => {
            // These should be handled during type checking/resolution.
            // If they reach MIR lowering, they are being used as values incorrectly.
            Err(LoweringError::unsupported_expression(
                "Type declarations cannot be used as expressions".to_string(),
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
            emit_runtime_to_string(ctx, rt::BOOL_TO_STRING, call_args, span)
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
            emit_runtime_to_string(ctx, rt::FLOAT_TO_STRING, call_args, span)
        }
        TypeKind::Int
        | TypeKind::I64
        | TypeKind::U64
        | TypeKind::I32
        | TypeKind::I16
        | TypeKind::I8
        | TypeKind::U32
        | TypeKind::U16
        | TypeKind::U8
        | TypeKind::I128
        | TypeKind::U128
        | TypeKind::Error => {
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
            emit_runtime_to_string(ctx, rt::INT_TO_STRING, call_args, span)
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
        ty: Type::new(TypeKind::Identifier, *span),
        literal: Literal::Identifier(runtime_fn.to_string()),
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
