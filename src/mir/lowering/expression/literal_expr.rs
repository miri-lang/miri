// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;

/// Infer a literal's MIR type when the type checker has none (preserving the
/// specific integer/float width carried by the literal).
fn infer_literal_type(lit: &crate::ast::literal::Literal, span: crate::error::syntax::Span) -> Type {
    use crate::ast::literal::{FloatLiteral, Literal};
    match lit {
        Literal::Integer(int_lit) => infer_integer_literal_type(int_lit, span),
        Literal::Boolean(_) => Type::new(TypeKind::Boolean, span),
        // Regex literals are represented as strings internally.
        Literal::String(_) | Literal::Regex(_) => Type::new(TypeKind::String, span),
        Literal::Float(FloatLiteral::F32(_)) => Type::new(TypeKind::F32, span),
        Literal::Float(FloatLiteral::F64(_)) => Type::new(TypeKind::F64, span),
        Literal::Identifier(_) => Type::new(TypeKind::Identifier, span),
        // `None` is the unit/absent value; use Void.
        Literal::None => Type::new(TypeKind::Void, span),
    }
}

/// Map an integer literal to its width-specific MIR type.
fn infer_integer_literal_type(
    int_lit: &crate::ast::literal::IntegerLiteral,
    span: crate::error::syntax::Span,
) -> Type {
    use crate::ast::literal::IntegerLiteral;
    match int_lit {
        IntegerLiteral::I8(_) => Type::new(TypeKind::I8, span),
        IntegerLiteral::I16(_) => Type::new(TypeKind::I16, span),
        IntegerLiteral::I32(_) => Type::new(TypeKind::I32, span),
        IntegerLiteral::I64(_) => Type::new(TypeKind::I64, span),
        IntegerLiteral::I128(_) => Type::new(TypeKind::I128, span),
        IntegerLiteral::U8(_) => Type::new(TypeKind::U8, span),
        IntegerLiteral::U16(_) => Type::new(TypeKind::U16, span),
        IntegerLiteral::U32(_) => Type::new(TypeKind::U32, span),
        IntegerLiteral::U64(_) => Type::new(TypeKind::U64, span),
        IntegerLiteral::U128(_) => Type::new(TypeKind::U128, span),
    }
}

pub(crate) fn lower_literal_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Literal(lit) = &expr.node else {
        unreachable!()
    };
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
