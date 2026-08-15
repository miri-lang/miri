// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

pub(crate) fn lower_identifier_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        unreachable!()
    };
    if let Some(&local) = ctx.variable_map.get(name.as_str()) {
        return lower_local_identifier(ctx, local, expr, dest);
    }

    let constant = build_global_identifier_operand(ctx, name, expr);
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

/// Lower a reference to a local variable: copy into `dest`, else Move/Copy per
/// the variable's auto-copy semantics.
///
/// An aggregate that assigns bitwise is rebuilt rather than referenced, so the
/// result is independent of the variable it came from.
fn lower_local_identifier(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let source = Place::new(local);

    if let Some(d) = dest {
        if let Some(operand) =
            super::value_copy::copy_value_aggregate(ctx, &source, &ty, Some(d.clone()), expr.span)?
        {
            return Ok(operand);
        }
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(Operand::Copy(source))),
            span: expr.span,
        });
        return Ok(Operand::Copy(d));
    }
    if let Some(operand) =
        super::value_copy::copy_value_aggregate(ctx, &source, &ty, None, expr.span)?
    {
        return Ok(operand);
    }
    if ctx.is_type_auto_copy(&ty) {
        Ok(Operand::Copy(source))
    } else {
        Ok(Operand::Move(source))
    }
}

/// Build the constant operand for a global identifier. Non-function constants
/// with a known compile-time value are inlined as a literal; everything else
/// (functions, value-less constants, unknown globals) emits the symbol name —
/// preferring the original name for import aliases so the linker resolves it.
fn build_global_identifier_operand(
    ctx: &LoweringContext,
    name: &str,
    expr: &Expression,
) -> Operand {
    let identifier_const = |ident: String| {
        Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Identifier, expr.span),
            literal: crate::ast::literal::Literal::Identifier(ident),
        }))
    };
    let Some(info) = ctx.type_checker.global_scope().get(name) else {
        return identifier_const(name.to_string());
    };
    let emit_name = info.original_name.as_deref().unwrap_or(name).to_string();
    if info.is_constant && !matches!(info.ty.kind, TypeKind::Function(_)) {
        if let Some(lit) = &info.value {
            return Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: info.ty.clone(),
                literal: lit.clone(),
            }));
        }
    }
    identifier_const(emit_name)
}
