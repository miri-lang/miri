// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind,
};

use std::rc::Rc;

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_enumvalue_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // EnumValue constructs an enum variant via `::` syntax, e.g. `Option::Some(v)`.
    let ExpressionKind::EnumValue(enum_expr, args) = &expr.node else {
        unreachable!()
    };
    let invalid = || {
        LoweringError::unsupported_expression(
            "Invalid EnumValue expression structure".to_string(),
            expr.span,
        )
    };
    let ExpressionKind::Member(type_expr, variant_expr) = &enum_expr.node else {
        return Err(invalid());
    };
    let ExpressionKind::Identifier(..) = &type_expr.node else {
        return Err(invalid());
    };
    let ExpressionKind::Identifier(variant_name, _) = &variant_expr.node else {
        return Err(invalid());
    };
    let Some(type_name) = ctx.type_checker.type_identity_named_by(type_expr) else {
        return Err(invalid());
    };
    let Some(discriminant) = enum_variant_discriminant(ctx, type_name, variant_name) else {
        return Err(invalid());
    };

    emit_enum_aggregate(ctx, type_name, variant_name, discriminant, args, expr, dest)
}

/// Lower the variant args and emit the enum `Aggregate` into `dest` (or a temp).
fn emit_enum_aggregate(
    ctx: &mut LoweringContext,
    type_name: &str,
    variant_name: &str,
    discriminant: usize,
    args: &[Expression],
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let instance = resolve_type(ctx.type_checker, expr);
    let mut ops = vec![enum_discriminant_operand(discriminant, expr.span)];
    ops.extend(lower_enum_payload(
        ctx,
        (type_name, variant_name),
        &instance,
        args,
    )?);

    // DPS: use the caller-provided destination, else allocate a fresh temp.
    let target = match dest {
        Some(d) => d,
        None => Place::new(ctx.push_temp(instance, expr.span)),
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(
                AggregateKind::Enum(Rc::from(type_name), Rc::from(variant_name)),
                ops,
            ),
        ),
        span: expr.span,
    });
    Ok(Operand::Copy(target))
}

/// Lower the payload `args` of variant `variant` of the enum `enum_name`,
/// built at `instance`, each brought to the type its slot declares: a bare
/// value is wrapped where the slot is an optional, and an absent value is
/// given the slot's bytes, as a store into any optional slot does.
pub(crate) fn lower_enum_payload(
    ctx: &mut LoweringContext,
    (enum_name, variant): (&str, &str),
    instance: &Type,
    args: &[Expression],
) -> Result<Vec<Operand>, LoweringError> {
    let slots = enum_payload_types(ctx, enum_name, variant, instance);
    let mut ops = Vec::with_capacity(args.len());
    for (position, arg) in args.iter().enumerate() {
        let watermark = ctx.body.local_decls.len();
        let op = lower_expression(ctx, arg, None)?;
        let op = match slots.get(position) {
            Some(slot) => {
                let op_ty = crate::mir::lowering::dispatch::argument_type(ctx, &op, arg);
                let (op, _) = crate::mir::lowering::helpers::wrap_for_optional_slot(
                    ctx, op, op_ty, slot, watermark, arg.span,
                );
                op
            }
            None => op,
        };
        ops.push(op);
    }
    Ok(ops)
}

/// The types the payload slots of `variant` of the enum `enum_name` hold,
/// at the arguments `instance` writes the enum with.
fn enum_payload_types(
    ctx: &LoweringContext,
    enum_name: &str,
    variant: &str,
    instance: &Type,
) -> Vec<Type> {
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(enum_name)
    else {
        return Vec::new();
    };
    let Some(fields) = enum_def.variants.get(variant) else {
        return Vec::new();
    };
    let arguments: &[Expression] = if let TypeKind::Custom(_, Some(args)) = &instance.kind {
        args
    } else {
        &[]
    };
    let substitution: std::collections::HashMap<String, Type> = enum_def
        .generics
        .iter()
        .flatten()
        .zip(arguments)
        .filter_map(|(generic, arg)| {
            Some((
                generic.name.clone(),
                crate::mir::instantiation::type_argument(arg)?,
            ))
        })
        .collect();
    fields
        .iter()
        .map(|field| {
            let field = crate::mir::lowering::apply_generic_sub(field, &substitution);
            crate::mir::lowering::variable::canonical_declared_type(ctx.type_checker, &field)
        })
        .collect()
}

/// Find the discriminant index of `variant_name` within enum `type_name`.
fn enum_variant_discriminant(
    ctx: &LoweringContext,
    type_name: &str,
    variant_name: &str,
) -> Option<usize> {
    let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(type_name)
    else {
        return None;
    };
    enum_def
        .variants
        .iter()
        .position(|(name, _)| name.as_str() == variant_name)
}

/// Build the i32 discriminant constant operand for an enum aggregate.
fn enum_discriminant_operand(discriminant: usize, span: crate::error::syntax::Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            discriminant as i32,
        )),
    }))
}
