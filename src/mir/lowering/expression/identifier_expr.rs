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
        // If destination is provided, assign the variable to it
        if let Some(d) = dest {
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    d.clone(),
                    Rvalue::Use(Operand::Copy(Place::new(local))),
                ),
                span: expr.span,
            });
            Ok(Operand::Copy(d))
        } else {
            // Check if the type is auto-copy to determine Move vs Copy semantics
            let ty = ctx.body.local_decls[local.0].ty.clone();
            if ctx.is_type_auto_copy(&ty) {
                Ok(Operand::Copy(Place::new(local)))
            } else {
                Ok(Operand::Move(Place::new(local)))
            }
        }
    } else {
        // Check if this identifier is a global non-function constant with a known
        // compile-time value. Such constants (e.g. `const MAX = 5` from an imported
        // module) are never allocated as locals in the calling function, so they
        // must be inlined as a literal operand at every use site.
        //
        // Functions whose body is a single literal also have `is_constant = true` in
        // global_scope (for call-site optimisation), but they must NOT be inlined here
        // — they are still called through their symbol name.
        let constant = if let Some(info) = ctx.type_checker.global_scope.get(name.as_str()) {
            // When this symbol is an import alias (e.g. `use m.{add as plus}`),
            // emit the *original* name so the linker resolves to the right symbol.
            let emit_name = info
                .original_name
                .as_deref()
                .unwrap_or(name.as_str())
                .to_string();
            if info.is_constant && !matches!(info.ty.kind, TypeKind::Function(_)) {
                if let Some(lit) = &info.value {
                    Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: info.ty.clone(),
                        literal: lit.clone(),
                    }))
                } else {
                    Operand::Constant(Box::new(Constant {
                        span: expr.span,
                        ty: Type::new(TypeKind::Identifier, expr.span),
                        literal: crate::ast::literal::Literal::Identifier(emit_name),
                    }))
                }
            } else {
                Operand::Constant(Box::new(Constant {
                    span: expr.span,
                    ty: Type::new(TypeKind::Identifier, expr.span),
                    literal: crate::ast::literal::Literal::Identifier(emit_name),
                }))
            }
        } else {
            // Assume global function/identifier
            Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: Type::new(TypeKind::Identifier, expr.span),
                literal: crate::ast::literal::Literal::Identifier(name.clone()),
            }))
        };

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
}
