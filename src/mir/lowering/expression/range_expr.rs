// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Range syntax lowering: `a..b` and `a..=b` build the `Range<T>` struct the
//! prelude declares, one field per bound and one for whether the end is
//! included.

use crate::ast::expression::{Expression, ExpressionKind, RangeExpressionType};
use crate::ast::types::{
    range_element_expression, Type, TypeKind, RANGE_END_FIELD, RANGE_INCLUSIVE_FIELD,
    RANGE_START_FIELD, RANGE_TYPE_NAME,
};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{BinOp, Constant, Operand, Place};
use crate::type_checker::context::TypeDefinition;

use crate::mir::lowering::constructors::{coerce_to_field, lower_argument, store_struct_aggregate};
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

/// How many fields `Range<T>` declares: its start, its end and whether the
/// end is included.
const RANGE_FIELD_COUNT: usize = 3;

/// The positions of the fields of `Range<T>` in its declaration, which is
/// the order a range value lays them out in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RangeFields {
    pub start: usize,
    pub end: usize,
    pub inclusive: usize,
}

/// Where each field of `Range<T>` sits, read off the struct the prelude
/// declares. A program checked without the prelude has no such struct, and
/// one whose `Range` has other fields has the wrong one; a range value
/// cannot be built in either.
pub(crate) fn range_fields(
    ctx: &LoweringContext,
    span: Span,
) -> Result<RangeFields, LoweringError> {
    let Some(TypeDefinition::Struct(definition)) =
        ctx.type_checker.type_definitions().get(RANGE_TYPE_NAME)
    else {
        return Err(misshapen_range_declaration(span));
    };
    if definition.fields.len() != RANGE_FIELD_COUNT {
        return Err(misshapen_range_declaration(span));
    }
    let position = |field: &str| {
        definition
            .fields
            .iter()
            .position(|(name, _, _)| name == field)
            .ok_or_else(|| misshapen_range_declaration(span))
    };
    Ok(RangeFields {
        start: position(RANGE_START_FIELD)?,
        end: position(RANGE_END_FIELD)?,
        inclusive: position(RANGE_INCLUSIVE_FIELD)?,
    })
}

/// The one error for a `Range` declaration range syntax cannot build: absent,
/// or declaring other fields than the three it fills.
fn misshapen_range_declaration(span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirMissingStructField,
        format!(
            "range syntax needs `system.range` to declare `struct {RANGE_TYPE_NAME}<T>` \
             with the fields `{RANGE_START_FIELD}`, `{RANGE_END_FIELD}` and \
             `{RANGE_INCLUSIVE_FIELD}`"
        ),
        span,
    )
}

/// Lowers range syntax into the `Range<T>` it builds. The bounds are
/// evaluated left to right, each brought to the range's element type, and
/// stored by the declaration's field order.
pub(crate) fn lower_range_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Range(start_expr, end_expr, range_type) = &expr.node else {
        unreachable!()
    };
    let (end_expr, inclusive) = match (end_expr, written_inclusivity(range_type)) {
        (Some(end_expr), Some(inclusive)) => (end_expr, inclusive),
        // An iterable object written in a loop header is the value itself.
        (None, None) => return lower_expression(ctx, start_expr, dest),
        (None, Some(_)) | (Some(_), None) => return Err(misshapen_range_syntax(expr.span)),
    };
    let fields = range_fields(ctx, expr.span)?;
    let range_ty = ctx.resolved_type(expr);
    let element_ty = range_element_type(ctx, &range_ty, start_expr);
    let watermark = ctx.body.local_decls.len();
    let start = lower_bound(ctx, start_expr, &element_ty, watermark)?;
    let end = lower_bound(ctx, end_expr, &element_ty, watermark)?;
    let flag = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Boolean, expr.span),
        literal: crate::ast::literal::Literal::Boolean(inclusive),
    }));

    let mut operands: [Option<Operand>; RANGE_FIELD_COUNT] = [None, None, None];
    operands[fields.start] = Some(start);
    operands[fields.end] = Some(end);
    operands[fields.inclusive] = Some(flag);
    let operands = operands
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| misshapen_range_declaration(expr.span))?;

    let target = match dest {
        Some(d) => d,
        None => Place::new(ctx.push_temp(range_ty.clone(), expr.span)),
    };
    Ok(store_struct_aggregate(
        ctx,
        target,
        (range_ty, operands),
        watermark,
        expr.span,
    ))
}

/// A range written with a start but no end, or an iterable object with one:
/// the parser builds neither.
fn misshapen_range_syntax(span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirUnsupportedRangeType,
        "range syntax pairs an end with `..` or `..=` and none with an iterable",
        span,
    )
}

/// Whether range syntax of `range_type` includes its end, or `None` for an
/// iterable object written in a loop header, which builds no range.
fn written_inclusivity(range_type: &RangeExpressionType) -> Option<bool> {
    match range_type {
        RangeExpressionType::Exclusive => Some(false),
        RangeExpressionType::Inclusive => Some(true),
        RangeExpressionType::IterableObject => None,
    }
}

/// The comparison a value makes against the end of range syntax of
/// `range_type` to be inside it: below an excluded end, at or below an
/// included one. `None` for an iterable object, which is no range.
pub(crate) fn written_range_end_test(range_type: &RangeExpressionType) -> Option<BinOp> {
    written_inclusivity(range_type).map(|inclusive| if inclusive { BinOp::Le } else { BinOp::Lt })
}

/// The element type `T` of the `Range<T>` a range expression was recorded
/// at, or its start's type when the record names no element.
fn range_element_type(ctx: &LoweringContext, range_ty: &Type, start: &Expression) -> Type {
    match range_element_expression(&range_ty.kind) {
        Some(element) => ctx.resolved_argument_type(element),
        None => ctx.resolved_type(start),
    }
}

/// One bound of a range, lowered and brought to the range's element type.
fn lower_bound(
    ctx: &mut LoweringContext,
    bound: &Expression,
    element_ty: &Type,
    watermark: usize,
) -> Result<Operand, LoweringError> {
    let argument = lower_argument(ctx, bound)?;
    Ok(coerce_to_field(
        ctx, argument, element_ty, watermark, bound.span,
    ))
}
