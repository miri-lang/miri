// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::{LoweringError, LoweringErrorKind};
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

/// Lower an identifier in value position.
///
/// A global function name is a value here, not a call target, so it becomes a
/// closure over a forwarding thunk — see [`super::function_reference`]. Callee
/// position wants the bare symbol instead and uses
/// [`lower_identifier_symbol`].
pub(crate) fn lower_identifier_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        unreachable!()
    };
    if let Some(&local) = ctx.variable_map.get(name.as_str()) {
        if ctx.self_references.contains_key(&local) {
            return Ok(lower_self_reference_value(ctx, local, expr.span, dest));
        }
        return lower_local_identifier(ctx, local, expr, dest);
    }
    if let Some(value) = super::function_reference::try_lower_function_reference(
        ctx,
        expr,
        name.as_str(),
        dest.clone(),
    ) {
        return Ok(value);
    }
    lower_identifier_symbol(ctx, expr, dest)
}

/// Lower a nested function's own name, used as a value inside its body, into
/// `dest` (a fresh temporary of the function's type when `None`).
///
/// The local holds the function's closure pointer, borrowed from the
/// environment. The value escapes as an owned closure, so it takes its own
/// reference, released like any other closure value.
pub(crate) fn lower_self_reference_value(
    ctx: &mut LoweringContext,
    local: crate::mir::Local,
    span: crate::error::syntax::Span,
    dest: Option<Place>,
) -> Operand {
    let source = Place::new(local);
    let target = dest.unwrap_or_else(|| {
        let ty = ctx.self_references[&local].clone();
        Place::new(ctx.push_temp(ty, span))
    });
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::IncRef(source.clone()),
        span,
    });
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target.clone(), Rvalue::Use(Operand::Copy(source))),
        span,
    });
    Operand::Copy(target)
}

/// Lower an identifier to a local read, or to the bare symbol constant a direct
/// call needs as its callee.
pub(crate) fn lower_identifier_symbol(
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

    let constant = build_global_identifier_operand(ctx, name, expr)?;
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

/// Build the constant operand for a global identifier. A binding with a known
/// compile-time value is inlined as that literal; everything else (functions,
/// unknown globals) emits the symbol name — preferring the original name for
/// import aliases so the linker resolves it.
///
/// A module-level binding has no storage: nothing runs a module's top-level
/// statements, so its compile-time value is the only value it has. One without
/// such a value is refused rather than read as a symbol codegen cannot
/// materialize.
pub(crate) fn build_global_identifier_operand(
    ctx: &LoweringContext,
    name: &str,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    let identifier_const = |ident: String| {
        Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: Type::new(TypeKind::Identifier, expr.span),
            literal: crate::ast::literal::Literal::Identifier(ident),
        }))
    };
    let Some(info) = ctx.type_checker.global_scope().get(name) else {
        return Ok(identifier_const(name.to_string()));
    };
    if matches!(info.ty.kind, TypeKind::Function(_)) {
        return Ok(identifier_const(
            info.original_name.as_deref().unwrap_or(name).to_string(),
        ));
    }
    let has_fixed_value = info.is_constant || !info.mutable;
    match &info.value {
        Some(literal) if has_fixed_value => Ok(Operand::Constant(Box::new(Constant {
            span: expr.span,
            ty: info.ty.clone(),
            literal: literal.clone(),
        }))),
        _ if info.module_scope => Err(module_binding_without_value(name, expr)),
        _ => Ok(identifier_const(
            info.original_name.as_deref().unwrap_or(name).to_string(),
        )),
    }
}

/// The refusal for reading a module-level binding that has no compile-time
/// value.
fn module_binding_without_value(name: &str, expr: &Expression) -> LoweringError {
    LoweringError::new(
        LoweringErrorKind::Coded {
            code: DiagnosticCode::MirUndefinedVariable,
            message: format!(
                "module-level binding '{name}' has no value at run time: a module's top-level \
                 statements never run, so only a binding with a compile-time value can be read"
            ),
            help: Some(format!(
                "initialize '{name}' with a literal or a constant expression, or read it through \
                 a function that computes the value."
            )),
        },
        expr.span,
    )
}
