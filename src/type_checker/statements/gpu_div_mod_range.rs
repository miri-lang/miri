// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Static range diagnostic for GPU `/` and `%` operands.
//!
//! On the GPU, Miri's `int` lowers to a 32-bit WGSL `i32`. A `/` or `%` operand
//! that is a compile-time integer literal outside the signed 32-bit range is
//! silently narrowed to `i32` by the WGSL backend (its high bits are dropped),
//! so the kernel would divide by a different value than the source spells.
//! Rather than truncate silently, this pass rejects such an operand inside a
//! `forall` (GPU) kernel at type-check time with a cast/clamp suggestion.
//!
//! The rule is conservative: only an operand whose value is *provably* out of
//! `i32` range — a literal, or a negated literal — is flagged. A runtime value,
//! a captured scalar, or a representable literal passes; those are either safe
//! or undecidable at this stage.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::literal::Literal;
use crate::ast::operator::{BinaryOp, UnaryOp};
use crate::ast::statement::{Statement, StatementKind};
use crate::error::syntax::Span;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reject every `/` or `%` in a GPU `forall` body whose divisor or dividend
    /// is a literal provably outside the 32-bit range representable on the GPU.
    /// Called once per pass after its body is type-checked.
    pub(crate) fn check_gpu_div_mod_range(&mut self, body: &Statement) {
        self.walk_stmt_for_div_mod(body);
    }

    /// Recurse into every sub-statement that can hold a value expression,
    /// checking each for out-of-range GPU `/` and `%` operands.
    fn walk_stmt_for_div_mod(&mut self, stmt: &Statement) {
        match &stmt.node {
            StatementKind::Block(stmts) => {
                for inner in stmts {
                    self.walk_stmt_for_div_mod(inner);
                }
            }
            StatementKind::Expression(expr) => self.walk_expr_for_div_mod(expr),
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    if let Some(init) = &decl.initializer {
                        self.walk_expr_for_div_mod(init);
                    }
                }
            }
            StatementKind::Return(Some(expr)) => self.walk_expr_for_div_mod(expr),
            StatementKind::If(cond, then_branch, else_branch, _) => {
                self.walk_expr_for_div_mod(cond);
                self.walk_stmt_for_div_mod(then_branch);
                if let Some(else_branch) = else_branch {
                    self.walk_stmt_for_div_mod(else_branch);
                }
            }
            StatementKind::While(cond, body, _) => {
                self.walk_expr_for_div_mod(cond);
                self.walk_stmt_for_div_mod(body);
            }
            StatementKind::For(_, iter, body) | StatementKind::GpuFrame(_, iter, body) => {
                self.walk_expr_for_div_mod(iter);
                self.walk_stmt_for_div_mod(body);
            }
            StatementKind::Forall { iterable, body, .. } => {
                self.walk_expr_for_div_mod(iterable);
                self.walk_stmt_for_div_mod(body);
            }
            StatementKind::GpuFrameBlock(block) => self.walk_stmt_for_div_mod(block),
            _ => {}
        }
    }

    /// Recurse into every sub-expression, flagging the operands of each `/` or
    /// `%` and descending through the rest of the expression tree.
    fn walk_expr_for_div_mod(&mut self, expr: &Expression) {
        match &expr.node {
            ExpressionKind::Binary(lhs, op, rhs) => {
                if matches!(op, BinaryOp::Div | BinaryOp::Mod) {
                    self.check_div_mod_operand(lhs);
                    self.check_div_mod_operand(rhs);
                }
                self.walk_expr_for_div_mod(lhs);
                self.walk_expr_for_div_mod(rhs);
            }
            ExpressionKind::Logical(lhs, _, rhs) => {
                self.walk_expr_for_div_mod(lhs);
                self.walk_expr_for_div_mod(rhs);
            }
            ExpressionKind::Unary(_, inner)
            | ExpressionKind::Guard(_, inner)
            | ExpressionKind::NamedArgument(_, inner) => self.walk_expr_for_div_mod(inner),
            ExpressionKind::Assignment(lhs, _, rhs) => {
                match lhs.as_ref() {
                    LeftHandSideExpression::Identifier(e)
                    | LeftHandSideExpression::Index(e)
                    | LeftHandSideExpression::Member(e) => self.walk_expr_for_div_mod(e),
                }
                self.walk_expr_for_div_mod(rhs);
            }
            ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
                self.walk_expr_for_div_mod(cond);
                self.walk_expr_for_div_mod(then_e);
                if let Some(else_e) = else_opt {
                    self.walk_expr_for_div_mod(else_e);
                }
            }
            ExpressionKind::Range(start, end_opt, _) => {
                self.walk_expr_for_div_mod(start);
                if let Some(end) = end_opt {
                    self.walk_expr_for_div_mod(end);
                }
            }
            ExpressionKind::Member(base, _) => self.walk_expr_for_div_mod(base),
            ExpressionKind::Index(base, index) => {
                self.walk_expr_for_div_mod(base);
                self.walk_expr_for_div_mod(index);
            }
            ExpressionKind::Call(func, args) | ExpressionKind::EnumValue(func, args) => {
                self.walk_expr_for_div_mod(func);
                for arg in args {
                    self.walk_expr_for_div_mod(arg);
                }
            }
            ExpressionKind::Cast(value, _) => self.walk_expr_for_div_mod(value),
            ExpressionKind::List(exprs)
            | ExpressionKind::Set(exprs)
            | ExpressionKind::Tuple(exprs)
            | ExpressionKind::FormattedString(exprs) => {
                for e in exprs {
                    self.walk_expr_for_div_mod(e);
                }
            }
            ExpressionKind::Array(exprs, init) => {
                for e in exprs {
                    self.walk_expr_for_div_mod(e);
                }
                self.walk_expr_for_div_mod(init);
            }
            ExpressionKind::Map(entries) => {
                for (key, value) in entries {
                    self.walk_expr_for_div_mod(key);
                    self.walk_expr_for_div_mod(value);
                }
            }
            ExpressionKind::Block(_, _) | ExpressionKind::Match(_, _) => {
                self.walk_stmt_bearing_expr(expr)
            }
            _ => {}
        }
    }

    /// Recurse through the two expression shapes that embed statements — a block
    /// expression and a `match` expression — so nested `/` and `%` operands in
    /// their bodies are still reached.
    fn walk_stmt_bearing_expr(&mut self, expr: &Expression) {
        match &expr.node {
            ExpressionKind::Block(stmts, final_expr) => {
                for s in stmts {
                    self.walk_stmt_for_div_mod(s);
                }
                self.walk_expr_for_div_mod(final_expr);
            }
            ExpressionKind::Match(scrutinee, branches) => {
                self.walk_expr_for_div_mod(scrutinee);
                for branch in branches {
                    if let Some(guard) = &branch.guard {
                        self.walk_expr_for_div_mod(guard);
                    }
                    self.walk_stmt_for_div_mod(&branch.body);
                }
            }
            _ => {}
        }
    }

    /// Flag a single `/` or `%` operand that is a provably out-of-`i32`-range
    /// literal.
    fn check_div_mod_operand(&mut self, operand: &Expression) {
        if !is_gpu_div_mod_safe(operand) {
            self.report_div_mod_out_of_range(operand.span);
        }
    }

    /// Emit the out-of-range diagnostic with a cast/clamp fix-it suggestion.
    fn report_div_mod_out_of_range(&mut self, span: Span) {
        self.report_error_with_help(
            "GPU '/' or '%' operand is an integer literal outside the 32-bit range \
             representable on the GPU; the WGSL backend would silently narrow it to \
             i32 and divide by a different value"
                .to_string(),
            span,
            "cast or clamp the operand into i32 range (for example 'x as i32') before \
             the division, or use a value that fits in 32 bits"
                .to_string(),
        );
    }
}

/// True unless `operand` is a compile-time integer literal (optionally negated)
/// whose value falls outside the signed 32-bit range. Non-literal operands are
/// undecidable here and are treated as safe.
fn is_gpu_div_mod_safe(operand: &Expression) -> bool {
    match literal_i128_value(operand) {
        Some(value) => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
        None => true,
    }
}

/// The compile-time value of an integer literal or a negated integer literal,
/// as `i128`. Returns `None` for any other expression.
fn literal_i128_value(expr: &Expression) -> Option<i128> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => Some(int_lit.to_i128()),
        ExpressionKind::Unary(UnaryOp::Negate, inner) => {
            literal_i128_value(inner).and_then(|value| value.checked_neg())
        }
        _ => None,
    }
}
