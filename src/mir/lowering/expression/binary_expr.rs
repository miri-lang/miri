// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    BinOp, Constant, Discriminant, Local, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind, UnOp,
};
use crate::runtime_fns::rt;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

#[allow(clippy::too_many_arguments)]
fn try_lower_binary_trait_method(
    ctx: &mut LoweringContext,
    lhs: &Expression,
    lhs_op: Operand,
    rhs_op: Operand,
    expr: &Expression,
    dest: Option<Place>,
    op: &crate::ast::operator::BinaryOp,
    arg_watermark: usize,
) -> Result<Option<Operand>, LoweringError> {
    let Some(class_name) = binary_trait_class_name(ctx, lhs) else {
        return Ok(None);
    };
    let Some((method_name, negate)) = binary_op_trait_method(op) else {
        return Ok(None);
    };
    if !class_has_trait_method(ctx, &class_name, method_name) {
        return Ok(None);
    }

    let call = BinTraitCall {
        lhs_op,
        rhs_op,
        dest,
        arg_watermark,
    };
    emit_binary_trait_call(ctx, &class_name, method_name, negate, call, expr).map(Some)
}

/// The class name implementing a binary operator trait for the lhs type
/// (`String` or a user `Custom` type), else None.
fn binary_trait_class_name(ctx: &LoweringContext, lhs: &Expression) -> Option<String> {
    match &ctx.type_checker.get_type(lhs.id)?.kind {
        TypeKind::String => Some(crate::ast::types::STRING_TYPE_NAME.to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        _ => None,
    }
}

/// Map a binary operator to its trait method name and whether to negate the
/// result (`Add→concat`, `Mul→repeat`, `Equal→equals`, `NotEqual→!equals`).
fn binary_op_trait_method(op: &crate::ast::operator::BinaryOp) -> Option<(&'static str, bool)> {
    match op {
        crate::ast::operator::BinaryOp::Add => Some(("concat", false)),
        crate::ast::operator::BinaryOp::Mul => Some(("repeat", false)),
        crate::ast::operator::BinaryOp::Equal => Some(("equals", false)),
        crate::ast::operator::BinaryOp::NotEqual => Some(("equals", true)),
        _ => None,
    }
}

/// True when `class_name` is a class defining `method_name`.
fn class_has_trait_method(ctx: &LoweringContext, class_name: &str, method_name: &str) -> bool {
    matches!(
        ctx.type_checker.global_type_definitions.get(class_name),
        Some(crate::type_checker::context::TypeDefinition::Class(cd))
            if cd.methods.contains_key(method_name)
    )
}

/// The operands + bookkeeping for emitting a binary-operator trait call.
struct BinTraitCall {
    lhs_op: Operand,
    rhs_op: Operand,
    dest: Option<Place>,
    arg_watermark: usize,
}

/// Emit `Class_method(lhs, rhs, alloc?)`, negating the boolean result for `!=`.
fn emit_binary_trait_call(
    ctx: &mut LoweringContext,
    class_name: &str,
    method_name: &str,
    negate: bool,
    call: BinTraitCall,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    let mangled_name = format!("{}_{}", class_name, method_name);
    let (call_args, arg_locals) = build_trait_call_args(ctx, call.lhs_op, call.rhs_op);

    let return_ty = match ctx.type_checker.global_type_definitions.get(class_name) {
        Some(crate::type_checker::context::TypeDefinition::Class(cd)) => {
            cd.methods[method_name].return_type.clone()
        }
        _ => unreachable!(),
    };
    let func_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(mangled_name),
    }));

    if negate {
        return_negated_method_call(
            ctx,
            func_op,
            call_args,
            arg_locals,
            return_ty,
            expr,
            call.dest,
            call.arg_watermark,
        )
    } else {
        return_method_call(
            ctx,
            func_op,
            call_args,
            arg_locals,
            return_ty,
            expr,
            call.dest,
            call.arg_watermark,
        )
    }
}

/// Build `[lhs, rhs, alloc?]` and the list of arg locals (for temp cleanup).
fn build_trait_call_args(
    ctx: &LoweringContext,
    lhs_op: Operand,
    rhs_op: Operand,
) -> (Vec<Operand>, Vec<Local>) {
    let mut call_args = vec![lhs_op, rhs_op];
    if let Some(&al) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(al)));
    }
    let arg_locals: Vec<Local> = call_args.iter().filter_map(operand_local).collect();
    (call_args, arg_locals)
}

/// The local backing a place operand, if any.
fn operand_local(op: &Operand) -> Option<Local> {
    match op {
        Operand::Copy(p) | Operand::Move(p) => Some(p.local),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn return_negated_method_call(
    ctx: &mut LoweringContext,
    func_op: Operand,
    call_args: Vec<Operand>,
    arg_locals: Vec<crate::mir::place::Local>,
    return_ty: Type,
    expr: &Expression,
    dest: Option<Place>,
    arg_watermark: usize,
) -> Result<Operand, LoweringError> {
    let eq_temp = ctx.push_temp(return_ty.clone(), expr.span);
    let after_eq_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args,
            out_args: Vec::new(),
            destination: Place::new(eq_temp),
            target: Some(after_eq_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(after_eq_bb);

    for &local in &arg_locals {
        if local != eq_temp {
            ctx.emit_temp_drop(local, arg_watermark, expr.span);
        }
    }

    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target,
            Rvalue::UnaryOp(UnOp::Not, Box::new(Operand::Copy(Place::new(eq_temp)))),
        ),
        span: expr.span,
    });
    Ok(ret_op)
}

#[allow(clippy::too_many_arguments)]
fn return_method_call(
    ctx: &mut LoweringContext,
    func_op: Operand,
    call_args: Vec<Operand>,
    arg_locals: Vec<crate::mir::place::Local>,
    return_ty: Type,
    expr: &Expression,
    dest: Option<Place>,
    arg_watermark: usize,
) -> Result<Operand, LoweringError> {
    let (destination, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(return_ty, expr.span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };
    let dest_local = destination.local;
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args,
            out_args: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);

    for &local in &arg_locals {
        if local != dest_local {
            ctx.emit_temp_drop(local, arg_watermark, expr.span);
        }
    }

    Ok(ret_op)
}

/// Pick the runtime membership-test function for the collection `rhs`.
fn resolve_contains_fn(ctx: &LoweringContext, rhs: &Expression) -> &'static str {
    match ctx.type_checker.get_type(rhs.id).map(|t| &t.kind) {
        Some(TypeKind::Set(_)) | Some(TypeKind::Map(_, _)) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        Some(TypeKind::Custom(name, _))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Set) =>
        {
            rt::SET_CONTAINS
        }
        Some(TypeKind::Custom(name, _))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
        {
            rt::MAP_CONTAINS_KEY
        }
        _ => "__contains",
    }
}

fn lower_in_operator(
    ctx: &mut LoweringContext,
    lhs: &Expression,
    rhs: &Expression,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let lhs_op = lower_expression(ctx, lhs, None)?;
    let rhs_op = lower_expression(ctx, rhs, None)?;

    let result_ty = Type::new(TypeKind::Boolean, expr.span);
    let (destination, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    let fn_name = resolve_contains_fn(ctx, rhs);
    let contains_fn = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(fn_name.to_string()),
    }));

    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: contains_fn,
            args: vec![rhs_op, lhs_op], // (collection, element)
            out_args: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);

    Ok(ret_op)
}

fn lower_option_equality(
    ctx: &mut LoweringContext,
    lhs_op: Operand,
    rhs_op: Operand,
    expr: &Expression,
    dest: Option<Place>,
    op: &crate::ast::operator::BinaryOp,
) -> Result<Operand, LoweringError> {
    let is_eq = matches!(op, crate::ast::operator::BinaryOp::Equal);
    let result_local = ctx.push_temp(Type::new(TypeKind::Boolean, expr.span), expr.span);
    let final_bb = ctx.new_basic_block();

    emit_option_ptr_eq_check(
        ctx,
        &lhs_op,
        &rhs_op,
        result_local,
        is_eq,
        final_bb,
        expr.span,
    );
    emit_option_lhs_null_check(
        ctx,
        &lhs_op,
        &rhs_op,
        result_local,
        is_eq,
        final_bb,
        expr.span,
    );
    emit_option_inner_compare(
        ctx,
        lhs_op,
        rhs_op,
        result_local,
        is_eq,
        final_bb,
        expr.span,
    );

    ctx.set_current_block(final_bb);
    finalize_option_comparison(ctx, result_local, dest, expr.span)
}

fn emit_option_ptr_eq_check(
    ctx: &mut LoweringContext,
    lhs_op: &Operand,
    rhs_op: &Operand,
    result_local: Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: crate::error::syntax::Span,
) {
    let ptr_eq_bb = ctx.new_basic_block();
    let check_null_bb = ctx.new_basic_block();
    let ptr_eq_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(ptr_eq_local),
            Rvalue::BinaryOp(
                BinOp::Eq,
                Box::new(lhs_op.clone()),
                Box::new(rhs_op.clone()),
            ),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(ptr_eq_local)),
            targets: vec![(Discriminant::bool_true(), ptr_eq_bb)],
            otherwise: check_null_bb,
        },
        span,
    ));

    ctx.set_current_block(ptr_eq_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: crate::ast::literal::Literal::Boolean(is_eq),
            }))),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));

    ctx.set_current_block(check_null_bb);
}

fn emit_option_lhs_null_check(
    ctx: &mut LoweringContext,
    lhs_op: &Operand,
    rhs_op: &Operand,
    result_local: Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: crate::error::syntax::Span,
) {
    let null_val = Operand::Constant(Box::new(Constant {
        span,
        ty: lhs_op.ty(&ctx.body).clone(),
        literal: crate::ast::literal::Literal::None,
    }));
    let lhs_null_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(lhs_null_local),
            Rvalue::BinaryOp(BinOp::Eq, Box::new(lhs_op.clone()), Box::new(null_val)),
        ),
        span,
    });

    let lhs_was_null_bb = ctx.new_basic_block();
    let check_rhs_null_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(lhs_null_local)),
            targets: vec![(Discriminant::bool_true(), lhs_was_null_bb)],
            otherwise: check_rhs_null_bb,
        },
        span,
    ));

    ctx.set_current_block(lhs_was_null_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: crate::ast::literal::Literal::Boolean(!is_eq),
            }))),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));

    ctx.set_current_block(check_rhs_null_bb);
    let null_val2 = Operand::Constant(Box::new(Constant {
        span,
        ty: rhs_op.ty(&ctx.body).clone(),
        literal: crate::ast::literal::Literal::None,
    }));
    let rhs_null_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(rhs_null_local),
            Rvalue::BinaryOp(BinOp::Eq, Box::new(rhs_op.clone()), Box::new(null_val2)),
        ),
        span,
    });

    let rhs_was_null_bb = ctx.new_basic_block();
    let compare_inner_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(rhs_null_local)),
            targets: vec![(Discriminant::bool_true(), rhs_was_null_bb)],
            otherwise: compare_inner_bb,
        },
        span,
    ));

    ctx.set_current_block(rhs_was_null_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: crate::ast::literal::Literal::Boolean(!is_eq),
            }))),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));

    ctx.set_current_block(compare_inner_bb);
}

fn emit_option_inner_compare(
    ctx: &mut LoweringContext,
    lhs_op: Operand,
    rhs_op: Operand,
    result_local: Local,
    is_eq: bool,
    final_bb: crate::mir::BasicBlock,
    span: crate::error::syntax::Span,
) {
    let lhs_place = crate::mir::lowering::helpers::ensure_place(ctx, lhs_op, span);
    let mut lhs_inner = lhs_place;
    lhs_inner.projection.push(crate::mir::PlaceElem::Field(0));

    let rhs_place = crate::mir::lowering::helpers::ensure_place(ctx, rhs_op, span);
    let mut rhs_inner = rhs_place;
    rhs_inner.projection.push(crate::mir::PlaceElem::Field(0));

    let bin_op = if is_eq { BinOp::Eq } else { BinOp::Ne };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::BinaryOp(
                bin_op,
                Box::new(Operand::Copy(lhs_inner)),
                Box::new(Operand::Copy(rhs_inner)),
            ),
        ),
        span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        span,
    ));
}

fn finalize_option_comparison(
    ctx: &mut LoweringContext,
    result_local: Local,
    dest: Option<Place>,
    span: crate::error::syntax::Span,
) -> Result<Operand, LoweringError> {
    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        (
            Place::new(result_local),
            Operand::Copy(Place::new(result_local)),
        )
    };

    if target.local != result_local {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                target,
                Rvalue::Use(Operand::Copy(Place::new(result_local))),
            ),
            span,
        });
    }

    Ok(ret_op)
}

/// Map AST binary operators to MIR BinOp, or error if unsupported.
fn op_to_binop(
    op: &crate::ast::operator::BinaryOp,
    expr_span: crate::error::syntax::Span,
) -> Result<BinOp, LoweringError> {
    match op {
        crate::ast::operator::BinaryOp::Add => Ok(BinOp::Add),
        crate::ast::operator::BinaryOp::Sub => Ok(BinOp::Sub),
        crate::ast::operator::BinaryOp::Mul => Ok(BinOp::Mul),
        crate::ast::operator::BinaryOp::Div => Ok(BinOp::Div),
        crate::ast::operator::BinaryOp::Mod => Ok(BinOp::Rem),
        crate::ast::operator::BinaryOp::BitwiseAnd => Ok(BinOp::BitAnd),
        crate::ast::operator::BinaryOp::BitwiseOr => Ok(BinOp::BitOr),
        crate::ast::operator::BinaryOp::BitwiseXor => Ok(BinOp::BitXor),
        crate::ast::operator::BinaryOp::Equal => Ok(BinOp::Eq),
        crate::ast::operator::BinaryOp::NotEqual => Ok(BinOp::Ne),
        crate::ast::operator::BinaryOp::LessThan => Ok(BinOp::Lt),
        crate::ast::operator::BinaryOp::LessThanEqual => Ok(BinOp::Le),
        crate::ast::operator::BinaryOp::GreaterThan => Ok(BinOp::Gt),
        crate::ast::operator::BinaryOp::GreaterThanEqual => Ok(BinOp::Ge),
        _ => Err(LoweringError::unsupported_operator(
            format!("{:?}", op),
            expr_span,
        )),
    }
}

/// Determine result type for a binary operation.
fn binary_result_type(
    ctx: &LoweringContext,
    op: &crate::ast::operator::BinaryOp,
    expr: &Expression,
) -> Type {
    match op {
        crate::ast::operator::BinaryOp::Equal
        | crate::ast::operator::BinaryOp::NotEqual
        | crate::ast::operator::BinaryOp::LessThan
        | crate::ast::operator::BinaryOp::LessThanEqual
        | crate::ast::operator::BinaryOp::GreaterThan
        | crate::ast::operator::BinaryOp::GreaterThanEqual => {
            Type::new(TypeKind::Boolean, expr.span)
        }
        _ => resolve_type(ctx.type_checker, expr),
    }
}

pub(crate) fn lower_binary_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Binary(lhs, op, rhs) = &expr.node else {
        unreachable!()
    };

    if matches!(op, crate::ast::operator::BinaryOp::In) {
        return lower_in_operator(ctx, lhs, rhs, expr, dest);
    }

    let arg_watermark = ctx.body.local_decls.len();
    let lhs_op = lower_expression(ctx, lhs, None)?;
    let rhs_op = lower_expression(ctx, rhs, None)?;

    if is_option_equality(ctx, lhs, op) {
        return lower_option_equality(ctx, lhs_op, rhs_op, expr, dest, op);
    }

    if let Some(result) = try_lower_binary_trait_method(
        ctx,
        lhs,
        lhs_op.clone(),
        rhs_op.clone(),
        expr,
        dest.clone(),
        op,
        arg_watermark,
    )? {
        return Ok(result);
    }

    emit_binary_op(ctx, op, lhs_op, rhs_op, expr, dest)
}

/// True when comparing an `Option` with `==`/`!=` (handled specially).
fn is_option_equality(
    ctx: &LoweringContext,
    lhs: &Expression,
    op: &crate::ast::operator::BinaryOp,
) -> bool {
    ctx.type_checker.get_type(lhs.id).is_some_and(|lhs_ty| {
        matches!(&lhs_ty.kind, TypeKind::Option(_))
            && matches!(
                op,
                crate::ast::operator::BinaryOp::Equal | crate::ast::operator::BinaryOp::NotEqual
            )
    })
}

/// Emit a plain `BinaryOp` rvalue into `dest` (or a fresh temp).
fn emit_binary_op(
    ctx: &mut LoweringContext,
    op: &crate::ast::operator::BinaryOp,
    lhs_op: Operand,
    rhs_op: Operand,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let bin_op = op_to_binop(op, expr.span)?;
    let result_ty = binary_result_type(ctx, op, expr);

    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target,
            Rvalue::BinaryOp(bin_op, Box::new(lhs_op), Box::new(rhs_op)),
        ),
        span: expr.span,
    });
    Ok(ret_op)
}
