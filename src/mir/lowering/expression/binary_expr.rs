// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    BinOp, Constant, Local, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind, UnOp,
};
use crate::runtime_fns::rt;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

/// The method a type defines to say how its values sort.
const ORDERING_METHOD_NAME: &str = "compare";

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
    try_lower_operator_trait_call(
        ctx,
        class_name,
        op,
        OperatorOperands { lhs_op, rhs_op },
        expr,
        dest,
        arg_watermark,
    )
}

/// The two values an operator is applied to.
pub(crate) struct OperatorOperands {
    pub lhs_op: Operand,
    pub rhs_op: Operand,
}

/// Lower `op` as a call to the trait method `class_name` defines for it, or
/// None when the operator has no trait method or the class does not define it.
///
/// Both spellings of an operator reach this: the binary expression an author
/// writes, and the comparison a parameter guard emits against the parameter.
/// Routing them through one function is what keeps a guard on a `String` from
/// comparing addresses while the same comparison in the body compares content.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_lower_operator_trait_call(
    ctx: &mut LoweringContext,
    class_name: &str,
    op: &crate::ast::operator::BinaryOp,
    operands: OperatorOperands,
    expr: &Expression,
    dest: Option<Place>,
    arg_watermark: usize,
) -> Result<Option<Operand>, LoweringError> {
    let Some((method_name, result)) = binary_op_trait_method(op) else {
        return Ok(None);
    };
    if !class_has_trait_method(ctx, class_name, method_name) {
        return Ok(None);
    }

    let call = BinTraitCall {
        lhs_op: operands.lhs_op,
        rhs_op: operands.rhs_op,
        dest,
        arg_watermark,
    };
    emit_binary_trait_call(ctx, class_name, method_name, result, call, expr).map(Some)
}

/// The class name implementing a binary operator trait for the lhs type
/// (`String` or a user `Custom` type), else None.
fn binary_trait_class_name<'tc>(ctx: &LoweringContext<'tc>, lhs: &Expression) -> Option<&'tc str> {
    operator_trait_class_name(&ctx.type_checker.get_type(lhs.id)?.kind)
}

/// The class name whose operator-trait methods apply to values of `kind`.
/// Returning `Option<&str>` avoids heap allocations during binary operator trait resolution.
pub(crate) fn operator_trait_class_name(kind: &TypeKind) -> Option<&str> {
    match kind {
        TypeKind::String => Some(crate::ast::types::STRING_TYPE_NAME),
        TypeKind::Custom(name, _) => Some(name.as_str()),
        _ => None,
    }
}

/// How the operator reads the value its trait method returned.
enum TraitResult {
    /// The method's result is the operator's result (`+`, `*`, `==`).
    AsReturned,
    /// The operator is the negation of the method's boolean result (`!=`).
    Negated,
    /// The operator compares the method's `int` result against zero, which is
    /// how one `compare` answers all four ordering operators.
    AgainstZero(BinOp),
}

/// Map a binary operator to its trait method name and how the operator reads
/// that method's result (`Add→concat`, `Mul→repeat`, `Equal→equals`,
/// `NotEqual→!equals`, ordering→`compare` against zero).
fn binary_op_trait_method(
    op: &crate::ast::operator::BinaryOp,
) -> Option<(&'static str, TraitResult)> {
    match op {
        crate::ast::operator::BinaryOp::Add => Some(("concat", TraitResult::AsReturned)),
        crate::ast::operator::BinaryOp::Mul => Some(("repeat", TraitResult::AsReturned)),
        crate::ast::operator::BinaryOp::Equal => Some(("equals", TraitResult::AsReturned)),
        crate::ast::operator::BinaryOp::NotEqual => Some(("equals", TraitResult::Negated)),
        crate::ast::operator::BinaryOp::LessThan => {
            Some((ORDERING_METHOD_NAME, TraitResult::AgainstZero(BinOp::Lt)))
        }
        crate::ast::operator::BinaryOp::LessThanEqual => {
            Some((ORDERING_METHOD_NAME, TraitResult::AgainstZero(BinOp::Le)))
        }
        crate::ast::operator::BinaryOp::GreaterThan => {
            Some((ORDERING_METHOD_NAME, TraitResult::AgainstZero(BinOp::Gt)))
        }
        crate::ast::operator::BinaryOp::GreaterThanEqual => {
            Some((ORDERING_METHOD_NAME, TraitResult::AgainstZero(BinOp::Ge)))
        }
        _ => None,
    }
}

/// True when `class_name` is a type (class or enum) defining `method_name`.
fn class_has_trait_method(ctx: &LoweringContext, class_name: &str, method_name: &str) -> bool {
    match ctx.type_checker.type_definitions().get(class_name) {
        Some(crate::type_checker::context::TypeDefinition::Class(cd)) => {
            cd.methods.contains_key(method_name)
        }
        Some(crate::type_checker::context::TypeDefinition::Enum(ed)) => {
            ed.methods.contains_key(method_name)
        }
        _ => false,
    }
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
    result: TraitResult,
    call: BinTraitCall,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    // Optimization: avoid format! overhead by allocating exact capacity.
    let mut mangled_name = String::with_capacity(class_name.len() + 1 + method_name.len());
    mangled_name.push_str(class_name);
    mangled_name.push('_');
    mangled_name.push_str(method_name);
    let (call_args, arg_locals) = build_trait_call_args(ctx, call.lhs_op, call.rhs_op);

    let return_ty = match ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(class_name)
    {
        Some(crate::type_checker::context::TypeDefinition::Class(cd)) => {
            cd.methods[method_name].return_type.clone()
        }
        Some(crate::type_checker::context::TypeDefinition::Enum(ed)) => {
            ed.methods[method_name].return_type.clone()
        }
        _ => unreachable!(),
    };
    let func_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier(mangled_name),
    }));

    let adapt = match result {
        TraitResult::AsReturned => {
            return return_method_call(
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
        TraitResult::Negated => ResultAdaptation::Negate,
        TraitResult::AgainstZero(bin_op) => ResultAdaptation::CompareToZero(bin_op),
    };

    return_adapted_method_call(
        ctx,
        func_op,
        call_args,
        arg_locals,
        AdaptedCall {
            return_ty,
            adapt,
            dest: call.dest,
            arg_watermark: call.arg_watermark,
        },
        expr,
    )
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

/// How the operator's result is computed from the value the trait method
/// returned, for the operators that do not return it unchanged.
enum ResultAdaptation {
    /// Logical negation of a boolean result.
    Negate,
    /// Comparison of an `int` result against zero.
    CompareToZero(BinOp),
}

/// The result handling for a trait call whose value the operator adapts.
struct AdaptedCall {
    return_ty: Type,
    adapt: ResultAdaptation,
    dest: Option<Place>,
    arg_watermark: usize,
}

/// Emit the trait call into a temp, then assign the operator's own result from
/// it: `not equals(...)` for `!=`, `compare(...) <op> 0` for the ordering
/// operators.
fn return_adapted_method_call(
    ctx: &mut LoweringContext,
    func_op: Operand,
    call_args: Vec<Operand>,
    arg_locals: Vec<crate::mir::place::Local>,
    call: AdaptedCall,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    let method_temp = ctx.push_temp(call.return_ty.clone(), expr.span);
    let after_call_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(method_temp),
            target: Some(after_call_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(after_call_bb);

    for &local in &arg_locals {
        if local != method_temp {
            ctx.emit_temp_drop(local, call.arg_watermark, expr.span);
        }
    }

    // The adapted result is a boolean whatever the method returned, so the
    // holding temp is typed from the operator rather than from the method.
    let result_ty = match call.adapt {
        ResultAdaptation::Negate => call.return_ty,
        ResultAdaptation::CompareToZero(_) => Type::new(TypeKind::Boolean, expr.span),
    };
    let (target, ret_op) = if let Some(d) = call.dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };
    let method_result = Operand::Copy(Place::new(method_temp));
    let rvalue = match call.adapt {
        ResultAdaptation::Negate => Rvalue::UnaryOp(UnOp::Not, Box::new(method_result)),
        ResultAdaptation::CompareToZero(bin_op) => Rvalue::BinaryOp(
            bin_op,
            Box::new(method_result),
            Box::new(zero_operand(expr)),
        ),
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target, rvalue),
        span: expr.span,
    });
    Ok(ret_op)
}

/// The integer zero an ordering result is compared against.
fn zero_operand(expr: &Expression) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Int, expr.span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I64(0)),
    }))
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
            arg_handles: Vec::new(),
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
            arg_handles: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);

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

    // Operator traits cover `+` (concat) and `*` (repeat) as well as `==`/`!=`,
    // so this dispatch must stay ahead of — and outside — the equality-only
    // structural path. A user-defined `equals` also wins over derived
    // structural comparison, which is what lets a type whose shape cannot be
    // compared structurally still define equality for itself.
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

    if is_equality_operator(op) {
        let structural = ctx
            .type_checker
            .get_type(lhs.id)
            .is_some_and(|ty| is_structural_equality_type(ctx, &ty.kind));
        if structural {
            return lower_structural_equality(ctx, lhs_op, rhs_op, expr, dest, op, arg_watermark);
        }
    }

    emit_binary_op(ctx, op, lhs_op, rhs_op, expr, dest)
}

/// True when the operator is `==` or `!=`.
fn is_equality_operator(op: &crate::ast::operator::BinaryOp) -> bool {
    matches!(
        op,
        crate::ast::operator::BinaryOp::Equal | crate::ast::operator::BinaryOp::NotEqual
    )
}

/// True when the type needs structural equality comparison.
fn is_structural_equality_type(ctx: &LoweringContext, kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Custom(name, _) => matches!(
            ctx.type_checker.type_definitions().get(name),
            Some(crate::type_checker::context::TypeDefinition::Enum(_))
                | Some(crate::type_checker::context::TypeDefinition::Struct(_))
        ),
        TypeKind::Result(_, _) | TypeKind::Option(_) => true,
        _ => false,
    }
}

/// Lower structural equality for enums, Result, and structs.
fn lower_structural_equality(
    ctx: &mut LoweringContext,
    lhs_op: Operand,
    rhs_op: Operand,
    expr: &Expression,
    dest: Option<Place>,
    op: &crate::ast::operator::BinaryOp,
    arg_watermark: usize,
) -> Result<Operand, LoweringError> {
    let is_eq = matches!(op, crate::ast::operator::BinaryOp::Equal);

    let ExpressionKind::Binary(lhs, _, _) = &expr.node else {
        return Err(LoweringError::unsupported_expression(
            "structural equality: invalid binary expression structure".to_string(),
            expr.span,
        ));
    };

    let Some(lhs_ty) = ctx.type_checker.get_type(lhs.id) else {
        return Err(LoweringError::unsupported_expression(
            "structural equality: cannot determine type".to_string(),
            expr.span,
        ));
    };

    let operand_locals: Vec<Local> = [&lhs_op, &rhs_op]
        .iter()
        .filter_map(|o| operand_local(o))
        .collect();

    let comparison_result =
        crate::mir::lowering::expression::structural_equality::emit_structural_equality(
            ctx,
            expr.span,
            &lhs_ty.kind,
            lhs_op,
            rhs_op,
            is_eq,
        )?;

    // The comparison branches on discriminants and payloads but converges
    // before returning, so the current block dominates every one of its exits.
    // Releasing the compared values here therefore covers each path exactly
    // once; releasing inside a branch would miss the paths that skip it.
    for local in operand_locals {
        if local != comparison_result {
            ctx.emit_temp_drop(local, arg_watermark, expr.span);
        }
    }

    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        (
            Place::new(comparison_result),
            Operand::Copy(Place::new(comparison_result)),
        )
    };

    if target.local != comparison_result {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                target,
                Rvalue::Use(Operand::Copy(Place::new(comparison_result))),
            ),
            span: expr.span,
        });
    }

    Ok(ret_op)
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
