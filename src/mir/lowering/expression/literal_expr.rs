// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind, REGEX_TYPE_NAME};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};
use crate::type_checker::expressions::literals::build_regex_pattern;

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
        Literal::Regex(_) => TypeKind::Custom(REGEX_TYPE_NAME.into(), None),
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

    // Regex literals are lowered to calls to `Regex.from_validated_pattern()`.
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

/// Lower a regex literal by calling Regex.from_validated_pattern().
///
/// Takes the expression id of the call site so that type resolution works correctly.
/// Always materializes to a temp or provided destination to ensure consistent ABI.
pub(crate) fn lower_regex_from_token(
    ctx: &mut LoweringContext,
    token: &crate::lexer::token::RegexToken,
    span: crate::error::syntax::Span,
    call_expr_id: usize,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    use crate::ast::expression::{Expression as AstExpr, ExpressionKind as AstExprKind};
    use crate::mir::lowering::dispatch::lower_call;

    // Build the pattern string with flags applied as a prefix.
    let pattern = build_regex_pattern(
        &token.body,
        token.ignore_case,
        token.multiline,
        token.dot_all,
        token.unicode,
    );

    // Construct synthetic AST expressions for Regex.from_validated_pattern(pattern)
    let class_expr = AstExpr {
        id: 0,
        node: AstExprKind::Identifier(REGEX_TYPE_NAME.to_string(), None),
        span,
    };

    let method_expr = AstExpr {
        id: 0,
        node: AstExprKind::Identifier("from_validated_pattern".to_string(), None),
        span,
    };

    let member_expr = AstExpr {
        id: 0,
        node: AstExprKind::Member(Box::new(class_expr), Box::new(method_expr)),
        span,
    };

    let pattern_expr = AstExpr {
        id: 0,
        node: AstExprKind::Literal(Literal::String(pattern)),
        span,
    };

    // Allocate a temp if no destination provided, to ensure consistent ABI
    let actual_dest = if let Some(d) = dest {
        Some(d)
    } else {
        let regex_ty = Type::new(TypeKind::Custom(REGEX_TYPE_NAME.into(), None), span);
        let temp = ctx.push_temp(regex_ty, span);
        Some(Place::new(temp))
    };

    // Call the static method through the normal dispatch mechanism.
    // Use the real call_expr_id so type resolution works correctly.
    lower_call(
        ctx,
        &span,
        call_expr_id,
        &member_expr,
        &[pattern_expr],
        actual_dest,
    )
}

/// Lower a regex literal by calling Regex.from_validated_pattern().
fn lower_regex_literal(
    ctx: &mut LoweringContext,
    expr: &Expression,
    token: &crate::lexer::token::RegexToken,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    lower_regex_from_token(ctx, token, expr.span, expr.id, dest)
}
