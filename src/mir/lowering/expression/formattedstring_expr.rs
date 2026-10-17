// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::ast::format_spec::{self, Align, ParsedSpec, Presentation};
use crate::error::syntax::Span;
use crate::mir::lowering::constructors::int_constant;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{
    emit_runtime_to_string, emit_string_append, emit_string_concat, emit_to_string,
    lower_expression,
};
use crate::runtime_fns::rt;

pub(crate) fn lower_formattedstring_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::FormattedString(parts, specs) = &expr.node else {
        unreachable!()
    };

    if parts.is_empty() {
        return lower_empty_formattedstring(ctx, expr);
    }

    let parts_watermark = ctx.body.local_decls.len();

    let mut string_parts = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        // The type checker refused every spec it could not read, so one that
        // fails to parse here belongs to a program that never reaches codegen.
        let spec = specs
            .iter()
            .find(|(at, _)| *at == index)
            .and_then(|(_, spec)| format_spec::parse(&spec.text).ok());
        let has_spec = specs.iter().any(|(at, _)| *at == index);
        if let Some(integer) = (!has_spec)
            .then(|| lower_integer_part(ctx, part))
            .transpose()?
            .flatten()
        {
            string_parts.push(integer);
            continue;
        }
        let string_local =
            lower_formattedstring_part(ctx, part, (expr, spec.as_ref()), parts_watermark)?;
        string_parts.push(Part::Text(string_local));
    }

    let accumulator = concat_formattedstring_parts(ctx, &string_parts, expr, parts_watermark)?;

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(accumulator))),
            ),
            span: expr.span,
        });
        ctx.emit_temp_drop(accumulator, parts_watermark, expr.span);
        return Ok(Operand::Copy(d));
    }

    Ok(Operand::Copy(Place::new(accumulator)))
}

fn lower_empty_formattedstring(
    ctx: &mut LoweringContext,
    expr: &Expression,
) -> Result<Operand, LoweringError> {
    use crate::ast::literal::Literal;

    let ty = Type::new(TypeKind::String, expr.span);
    let temp = ctx.push_temp(ty.clone(), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty,
                literal: Literal::String(String::new()),
            }))),
        ),
        span: expr.span,
    });
    Ok(Operand::Copy(Place::new(temp)))
}

fn lower_formattedstring_part(
    ctx: &mut LoweringContext,
    part: &Expression,
    (parent_expr, spec): (&Expression, Option<&ParsedSpec>),
    parts_watermark: usize,
) -> Result<crate::mir::place::Local, LoweringError> {
    let part_op = lower_expression(ctx, part, None)?;

    let part_kind = ctx
        .type_checker
        .get_type(part.id)
        .map(|t| t.kind.clone())
        .unwrap_or_else(|| match &part_op {
            Operand::Constant(c) => c.ty.kind.clone(),
            Operand::Function(function) => function.ty.kind.clone(),
            Operand::Copy(p) | Operand::Move(p) => ctx.body.local_decls[p.local.0].ty.kind.clone(),
        });

    let string_source_local: Option<crate::mir::place::Local> = match &part_kind {
        TypeKind::String
        | TypeKind::Option(_)
        | TypeKind::Result(_, _)
        | TypeKind::Custom(_, _) => match &part_op {
            Operand::Copy(p) | Operand::Move(p) if p.local.0 >= parts_watermark => Some(p.local),
            _ => None,
        },
        _ => None,
    };

    let span = parent_expr.span;
    let string_local = match spec.and_then(|spec| spec.presentation) {
        Some(Presentation::Fixed) => {
            let precision = spec.and_then(|spec| spec.precision).unwrap_or(6);
            emit_fixed(ctx, part_op, precision, &span)?
        }
        Some(radix) => emit_radix(ctx, part_op, &part_kind, radix, &span)?,
        None => emit_to_string(ctx, part_op, &part_kind, &span)?,
    };

    if let Some(src) = string_source_local {
        if src != string_local {
            ctx.emit_temp_drop(src, parts_watermark, span);
        }
    }

    let Some(spec) = spec.filter(|spec| spec.width.is_some()) else {
        return Ok(string_local);
    };
    let padded = emit_pad(ctx, string_local, spec, is_number(&part_kind), &span)?;
    ctx.emit_temp_drop(string_local, parts_watermark, span);
    Ok(padded)
}

/// The value written in fixed point with `precision` decimals; an integer is
/// converted to a float first, as Python writes `{5:.1f}`.
fn emit_fixed(
    ctx: &mut LoweringContext,
    operand: Operand,
    precision: u32,
    span: &Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    let float = cast_to(ctx, operand, TypeKind::Float, span);
    let args = vec![
        Operand::Copy(Place::new(float)),
        int_constant(i64::from(precision), span),
    ];
    emit_runtime_to_string(ctx, rt::FORMAT_FIXED, args, span)
}

/// The integer written in the radix `presentation` names.
fn emit_radix(
    ctx: &mut LoweringContext,
    operand: Operand,
    kind: &TypeKind,
    presentation: Presentation,
    span: &Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    let unsigned = matches!(
        kind,
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64
    );
    let word = cast_to(ctx, operand, TypeKind::Int, span);
    let args = vec![
        Operand::Copy(Place::new(word)),
        int_constant(i64::from(presentation.radix().unwrap_or(10)), span),
        int_constant(i64::from(presentation == Presentation::UpperHex), span),
        int_constant(i64::from(unsigned), span),
    ];
    emit_runtime_to_string(ctx, rt::FORMAT_RADIX, args, span)
}

/// `text` padded to the spec's width. A number aligns right unless told
/// otherwise and anything else left; a leading `0` pads a number with zeros
/// after its sign, or, beside an explicit alignment, is the fill.
fn emit_pad(
    ctx: &mut LoweringContext,
    text: crate::mir::place::Local,
    spec: &ParsedSpec,
    number: bool,
    span: &Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    let (align, fill, zero) = match spec.align {
        None => (
            if number { Align::Right } else { Align::Left },
            spec.fill,
            spec.zero,
        ),
        Some(align) if spec.zero && spec.fill == ' ' => (align, '0', false),
        Some(align) => (align, spec.fill, false),
    };
    let align_code = match align {
        Align::Left => 0,
        Align::Right => 1,
        Align::Center => 2,
    };
    let args = vec![
        Operand::Copy(Place::new(text)),
        int_constant(i64::from(spec.width.unwrap_or(0)), span),
        int_constant(align_code, span),
        int_constant(i64::from(u32::from(fill)), span),
        int_constant(i64::from(zero), span),
    ];
    emit_runtime_to_string(ctx, rt::FORMAT_PAD, args, span)
}

/// A temp holding `operand` converted to `kind`.
fn cast_to(
    ctx: &mut LoweringContext,
    operand: Operand,
    kind: TypeKind,
    span: &Span,
) -> crate::mir::place::Local {
    let ty = Type::new(kind, *span);
    let temp = ctx.push_temp(ty.clone(), *span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Cast(Box::new(operand), ty)),
        span: *span,
    });
    temp
}

/// Whether a value of `kind` aligns right in a field, as a number does.
fn is_number(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
    )
}

/// One part of a formatted string, lowered.
#[derive(Clone, Copy)]
enum Part {
    /// A string holding the part's text.
    Text(crate::mir::place::Local),
    /// An integer of at most 64 bits, written without a format spec: its
    /// digits are appended straight onto the text built so far.
    Integer {
        value: crate::mir::place::Local,
        unsigned: bool,
    },
}

/// Lowers `part` into an integer part when it is an integer of at most 64
/// bits, widened to the word the runtime's formatters take; `None` for any
/// other part, left unlowered.
fn lower_integer_part(
    ctx: &mut LoweringContext,
    part: &Expression,
) -> Result<Option<Part>, LoweringError> {
    let Some(kind) = ctx.type_checker.get_type(part.id).map(|t| t.kind.clone()) else {
        return Ok(None);
    };
    let signed = matches!(
        kind,
        TypeKind::Int | TypeKind::I64 | TypeKind::I32 | TypeKind::I16 | TypeKind::I8
    );
    let unsigned = matches!(
        kind,
        TypeKind::U64 | TypeKind::U32 | TypeKind::U16 | TypeKind::U8
    );
    if !signed && !unsigned {
        return Ok(None);
    }
    let operand = lower_expression(ctx, part, None)?;
    let value = cast_to(ctx, operand, TypeKind::Int, &part.span);
    Ok(Some(Part::Integer { value, unsigned }))
}

/// The text of an integer part as a string of its own.
fn integer_text(
    ctx: &mut LoweringContext,
    value: crate::mir::place::Local,
    unsigned: bool,
    span: &Span,
) -> Result<crate::mir::place::Local, LoweringError> {
    let runtime_fn = if unsigned {
        rt::UINT_TO_STRING
    } else {
        rt::INT_TO_STRING
    };
    emit_runtime_to_string(
        ctx,
        runtime_fn,
        vec![Operand::Copy(Place::new(value))],
        span,
    )
}

fn concat_formattedstring_parts(
    ctx: &mut LoweringContext,
    string_parts: &[Part],
    parent_expr: &Expression,
    parts_watermark: usize,
) -> Result<crate::mir::place::Local, LoweringError> {
    let span = &parent_expr.span;
    let mut accumulator = match string_parts[0] {
        Part::Text(local) => local,
        Part::Integer { value, unsigned } => integer_text(ctx, value, unsigned, span)?,
    };
    for &next_part in &string_parts[1..] {
        let old_acc = accumulator;
        // A temp of this string's own is appended to in place; a part that is
        // a binding of the program is read, never taken, so the first step
        // copies it into a new string.
        let owns_accumulator = old_acc.0 >= parts_watermark;
        accumulator = match next_part {
            Part::Integer { value, unsigned } if owns_accumulator => {
                emit_integer_append(ctx, old_acc, value, unsigned, span)
            }
            Part::Integer { value, unsigned } => {
                let text = integer_text(ctx, value, unsigned, span)?;
                let joined = emit_string_concat(ctx, old_acc, text, span)?;
                ctx.emit_temp_drop(text, parts_watermark, *span);
                joined
            }
            Part::Text(text) => {
                let joined = if owns_accumulator {
                    emit_string_append(ctx, old_acc, text, span)
                } else {
                    emit_string_concat(ctx, old_acc, text, span)?
                };
                ctx.emit_temp_drop(text, parts_watermark, *span);
                joined
            }
        };
    }
    Ok(accumulator)
}

/// Appends the digits of the integer `value` holds onto the text `left`
/// holds, taking over `left`; returns the local holding the result.
fn emit_integer_append(
    ctx: &mut LoweringContext,
    left: crate::mir::place::Local,
    value: crate::mir::place::Local,
    unsigned: bool,
    span: &Span,
) -> crate::mir::place::Local {
    let runtime_fn = if unsigned {
        rt::STRING_APPEND_UINT
    } else {
        rt::STRING_APPEND_INT
    };
    let result = ctx.push_temp(Type::new(TypeKind::String, *span), *span);
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(crate::mir::Terminator::new(
        crate::mir::TerminatorKind::Call {
            func: Operand::runtime(runtime_fn, *span),
            args: vec![
                Operand::Move(Place::new(left)),
                Operand::Copy(Place::new(value)),
            ],
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(result),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    result
}
