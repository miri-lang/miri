// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};
use crate::type_checker::expressions::literals::build_regex_pattern;
use crate::type_checker::literal_hooks::{regex_literal, RegexLiteral};

use crate::mir::lowering::context::LoweringContext;

/// Map an integer literal to its width-specific `TypeKind`.
fn integer_literal_type_kind(int_lit: &crate::ast::literal::IntegerLiteral) -> TypeKind {
    use crate::ast::literal::IntegerLiteral;
    match int_lit {
        IntegerLiteral::I8(_) => TypeKind::I8,
        IntegerLiteral::I16(_) => TypeKind::I16,
        IntegerLiteral::I32(_) => TypeKind::I32,
        IntegerLiteral::I64(_) => TypeKind::I64,
        IntegerLiteral::I128(_) => TypeKind::I128,
        IntegerLiteral::U8(_) => TypeKind::U8,
        IntegerLiteral::U16(_) => TypeKind::U16,
        IntegerLiteral::U32(_) => TypeKind::U32,
        IntegerLiteral::U64(_) => TypeKind::U64,
        IntegerLiteral::U128(_) => TypeKind::U128,
    }
}

/// Map a literal to its `TypeKind`.
fn literal_type_kind(lit: &crate::ast::literal::Literal) -> TypeKind {
    use crate::ast::literal::{FloatLiteral, Literal};
    match lit {
        Literal::Integer(int_lit) => integer_literal_type_kind(int_lit),
        Literal::Boolean(_) => TypeKind::Boolean,
        Literal::String(_) => TypeKind::String,
        // A regex literal is built by a call (`lower_regex_literal`) and is
        // never typed from the literal alone.
        Literal::Regex(_) => TypeKind::Void,
        Literal::Float(FloatLiteral::F32(_)) => TypeKind::F32,
        Literal::Float(FloatLiteral::F64(_)) => TypeKind::F64,
        Literal::Identifier(_) => TypeKind::Identifier,
        Literal::None => TypeKind::Void,
    }
}

/// Infer a literal's MIR type when the type checker has none (preserving the
/// specific integer/float width carried by the literal).
fn infer_literal_type(
    lit: &crate::ast::literal::Literal,
    span: crate::error::syntax::Span,
) -> Type {
    Type::new(literal_type_kind(lit), span)
}

pub(crate) fn lower_literal_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Literal(lit) = &expr.node else {
        unreachable!()
    };

    // A regex literal is lowered to a call of the method marked `@regex_literal`.
    if let Literal::Regex(token) = lit {
        return lower_regex_literal(ctx, expr, token, dest);
    }

    // Prefer the type checker's resolved type for context-aware typing; only
    // fall back to inferring from the literal when it has none.
    let ty = match ctx.type_checker.get_type(expr.id) {
        Some(resolved) => resolved.clone(),
        None => infer_literal_type(lit, expr.span),
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

/// Lower a regex literal by calling the static method its class marks
/// `@regex_literal` ([`regex_literal`]) on the validated pattern, through the
/// ordinary call path, into `dest` or a new temporary.
///
/// Takes the expression id of the call site so that type resolution works correctly.
pub(crate) fn lower_regex_from_token(
    ctx: &mut LoweringContext,
    token: &crate::lexer::token::RegexToken,
    span: crate::error::syntax::Span,
    call_expr_id: usize,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    use crate::ast::expression::{Expression as AstExpr, ExpressionKind as AstExprKind};
    use crate::mir::lowering::dispatch::lower_call;

    let hook = regex_literal_hook(ctx, span)?;
    let pattern = build_regex_pattern(
        &token.body,
        token.ignore_case,
        token.multiline,
        token.dot_all,
        token.unicode,
    );
    let synthetic = |node| AstExpr { id: 0, node, span };
    let member_expr = synthetic(AstExprKind::Member(
        Box::new(synthetic(AstExprKind::Identifier(hook.class.clone(), None))),
        Box::new(synthetic(AstExprKind::Identifier(hook.constructor, None))),
    ));
    let pattern_expr = synthetic(AstExprKind::Literal(Literal::String(pattern)));

    let dest = dest.unwrap_or_else(|| {
        let regex_ty = Type::new(TypeKind::Custom(hook.class, None), span);
        Place::new(ctx.push_temp(regex_ty, span))
    });
    lower_call(
        ctx,
        &span,
        call_expr_id,
        &member_expr,
        &[pattern_expr],
        Some(dest),
    )
}

/// The class and methods regex literals go through; the type checker refused
/// any regex literal in a program declaring none.
pub(crate) fn regex_literal_hook(
    ctx: &LoweringContext,
    span: crate::error::syntax::Span,
) -> Result<RegexLiteral, LoweringError> {
    regex_literal(ctx.type_checker.type_definitions()).ok_or_else(|| {
        LoweringError::unsupported_expression(
            "a regex literal needs a class declaring a `@regex_literal` method".to_string(),
            span,
        )
    })
}

/// Lower a regex literal by calling the method marked `@regex_literal`.
fn lower_regex_literal(
    ctx: &mut LoweringContext,
    expr: &Expression,
    token: &crate::lexer::token::RegexToken,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    lower_regex_from_token(ctx, token, expr.span, expr.id, dest)
}
