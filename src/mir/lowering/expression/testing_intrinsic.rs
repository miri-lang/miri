// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-level lowering for the `system.testing` assertion intrinsics.
//!
//! The intrinsics `assert`, `assert_eq`, `assert_ne`, and `assert_panics` are
//! declared without bodies in `src/stdlib/system/testing.mi`. At call sites we
//! synthesize the failure-path MIR directly here so the runtime diagnostic
//! message can include the source file and line of the failing call without
//! requiring every assertion to thread a location parameter explicitly.
//!
//! The lowering is intentionally narrow: it dispatches on the resolved monomorphised
//! type of the receiver and emits a call to the matching runtime helper
//! (`miri_rt_assert_fail`, `miri_rt_assert_eq_fail`, `miri_rt_assert_ne_fail`,
//! `miri_rt_assert_panics`).

use crate::ast::expression::Expression;
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::{emit_to_string, lower_expression};
use crate::mir::place::Local;
use crate::mir::{
    BinOp, Constant, Discriminant, Operand, Place, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;

/// Name of every assertion intrinsic exported from `system.testing`. Used to
/// short-circuit `lower_call_expr` before its generic-mangling step.
pub(crate) fn is_testing_intrinsic(name: &str) -> bool {
    matches!(name, "assert" | "assert_eq" | "assert_ne" | "assert_panics")
}

/// Returns true if the named function was imported from `system.testing` in
/// the current compilation. Mirrors the `system.math` guard used by the math
/// intrinsics so that user code shadowing the assertion names with their own
/// functions is unaffected.
pub(crate) fn is_from_testing_module(ctx: &LoweringContext<'_>, name: &str) -> bool {
    ctx.type_checker
        .get_variable_module(name)
        .map(|m| m == "system.testing")
        .unwrap_or(false)
}

/// Lower a call to an assertion intrinsic. Returns the void-typed call result
/// in a fresh temp so the caller can treat it like any other expression.
pub(crate) fn lower_testing_intrinsic(
    ctx: &mut LoweringContext,
    expr: &Expression,
    name: &str,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let span = expr.span;

    match name {
        "assert" => lower_assert(ctx, span, args, dest),
        "assert_eq" => lower_assert_eq(ctx, expr, span, args, dest, AssertCmp::Eq),
        "assert_ne" => lower_assert_eq(ctx, expr, span, args, dest, AssertCmp::Ne),
        "assert_panics" => lower_assert_panics(ctx, span, args, dest),
        _ => unreachable!("non-testing intrinsic dispatched to testing lowering"),
    }
}

#[derive(Copy, Clone)]
enum AssertCmp {
    Eq,
    Ne,
}

// ----------------------------------------------------------------------------
// `assert(condition bool, message String = "")`
// ----------------------------------------------------------------------------

fn lower_assert(
    ctx: &mut LoweringContext,
    span: Span,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if args.is_empty() {
        return Err(LoweringError::unsupported_expression(
            "assert requires a boolean condition argument".to_string(),
            span,
        ));
    }
    let watermark = ctx.body.local_decls.len();
    let cond_op = lower_expression(ctx, &args[0], None)?;
    let msg_op = lower_optional_string_arg(ctx, args.get(1), span)?;
    let loc_op = build_location_operand(ctx, span);

    let fail_bb = ctx.new_basic_block();
    let after_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Discriminant::bool_true(), after_bb)],
            otherwise: fail_bb,
        },
        span,
    ));

    ctx.set_current_block(fail_bb);
    emit_call_no_return(
        ctx,
        span,
        rt::ASSERT_FAIL,
        vec![msg_op.clone(), loc_op.clone()],
        after_bb,
    );

    ctx.set_current_block(after_bb);
    drop_managed_operands(ctx, span, &[&msg_op, &loc_op], watermark);
    Ok(materialize_void(ctx, span, dest))
}

// ----------------------------------------------------------------------------
// `assert_eq<T>(actual T, expected T, message String = "")`
// `assert_ne<T>(a T, b T, message String = "")`
// ----------------------------------------------------------------------------

fn lower_assert_eq(
    ctx: &mut LoweringContext,
    expr: &Expression,
    span: Span,
    args: &[Expression],
    dest: Option<Place>,
    cmp: AssertCmp,
) -> Result<Operand, LoweringError> {
    if args.len() < 2 {
        return Err(LoweringError::unsupported_expression(
            "assert_eq / assert_ne require two value arguments".to_string(),
            span,
        ));
    }

    let actual_arg = &args[0];
    let expected_arg = &args[1];
    let ast_kind = resolve_value_kind(ctx, expr, actual_arg, expected_arg, span)?;

    let watermark = ctx.body.local_decls.len();
    let actual_op = lower_expression(ctx, actual_arg, None)?;
    let expected_op = lower_expression(ctx, expected_arg, None)?;

    // Prefer the AST-resolved kind unless it is unusable (e.g. a generic `T`
    // that the type-checker never folded down to a primitive — happens when
    // the value flows from an unmonomorphised function return). In that case
    // pull the concrete kind from the lowered operands, where the local's
    // declared type already carries the substituted value type.
    let value_kind = if kind_is_primitive(&ast_kind) {
        ast_kind
    } else {
        pick_concrete_kind(ctx, &actual_op, &expected_op, ast_kind)
    };
    let user_msg = lower_optional_string_arg(ctx, args.get(2), span)?;
    let loc_op = build_location_operand(ctx, span);

    let eq_local = emit_equality(
        ctx,
        span,
        &value_kind,
        actual_op.clone(),
        expected_op.clone(),
    )?;
    let cmp_op = Operand::Copy(Place::new(eq_local));

    let fail_bb = ctx.new_basic_block();
    let after_bb = ctx.new_basic_block();

    let (eq_target, ne_target) = match cmp {
        AssertCmp::Eq => (after_bb, fail_bb),
        AssertCmp::Ne => (fail_bb, after_bb),
    };

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: cmp_op,
            targets: vec![(Discriminant::bool_true(), eq_target)],
            otherwise: ne_target,
        },
        span,
    ));

    ctx.set_current_block(fail_bb);

    let expected_str_local =
        emit_value_to_quoted_string(ctx, &value_kind, expected_op.clone(), span)?;
    let actual_str_local = emit_value_to_quoted_string(ctx, &value_kind, actual_op.clone(), span)?;

    let expected_op_s = Operand::Copy(Place::new(expected_str_local));
    let actual_op_s = Operand::Copy(Place::new(actual_str_local));

    let (fail_fn, fail_args) = match cmp {
        AssertCmp::Eq => (
            rt::ASSERT_EQ_FAIL,
            vec![expected_op_s, actual_op_s, user_msg.clone(), loc_op.clone()],
        ),
        AssertCmp::Ne => (
            rt::ASSERT_NE_FAIL,
            vec![actual_op_s, user_msg.clone(), loc_op.clone()],
        ),
    };
    emit_call_no_return(ctx, span, fail_fn, fail_args, after_bb);

    ctx.set_current_block(after_bb);
    drop_managed_operands(
        ctx,
        span,
        &[&actual_op, &expected_op, &user_msg, &loc_op],
        watermark,
    );
    Ok(materialize_void(ctx, span, dest))
}

// ----------------------------------------------------------------------------
// `assert_panics(f fn(), expected String = "")`
// ----------------------------------------------------------------------------

fn lower_assert_panics(
    ctx: &mut LoweringContext,
    span: Span,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if args.is_empty() {
        return Err(LoweringError::unsupported_expression(
            "assert_panics requires a zero-arg closure argument".to_string(),
            span,
        ));
    }
    let watermark = ctx.body.local_decls.len();
    let closure_op = lower_expression(ctx, &args[0], None)?;
    let expected_op = lower_optional_string_arg(ctx, args.get(1), span)?;
    let loc_op = build_location_operand(ctx, span);

    let next_bb = ctx.new_basic_block();
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let func_op = identifier_constant(rt::ASSERT_PANICS, span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![closure_op.clone(), expected_op.clone(), loc_op.clone()],
            out_args: Vec::new(),
            destination: Place::new(void_temp),
            target: Some(next_bb),
        },
        span,
    ));

    ctx.set_current_block(next_bb);
    drop_managed_operands(ctx, span, &[&closure_op, &expected_op, &loc_op], watermark);
    Ok(materialize_void(ctx, span, dest))
}

/// Emit `StorageDead` for every managed temp local that backs one of these
/// operands and was created after `watermark`. Mirrors the cleanup pass that
/// `lower_direct_call` runs after a normal function call so the runtime
/// helpers receive balanced IncRef/DecRef pairs.
fn drop_managed_operands(
    ctx: &mut LoweringContext,
    span: Span,
    operands: &[&Operand],
    watermark: usize,
) {
    for op in operands {
        if let Operand::Copy(place) | Operand::Move(place) = op {
            ctx.emit_temp_drop(place.local, watermark, span);
        }
    }
}

// ----------------------------------------------------------------------------
// Shared helpers
// ----------------------------------------------------------------------------

/// Build an `Operand` carrying the call-site location string. Falls back to
/// `"line N"` when no source path is attached to the context (which is the
/// case for the in-memory test driver).
fn build_location_operand(ctx: &mut LoweringContext, span: Span) -> Operand {
    let location = ctx.format_span_location(span);
    string_literal_operand(ctx, span, location)
}

/// Lower an optional `String` argument, falling back to an empty literal when
/// the caller omitted it (default-argument case).
fn lower_optional_string_arg(
    ctx: &mut LoweringContext,
    arg: Option<&Expression>,
    span: Span,
) -> Result<Operand, LoweringError> {
    match arg {
        Some(expr) => lower_expression(ctx, expr, None),
        None => Ok(string_literal_operand(ctx, span, String::new())),
    }
}

/// Emit a runtime call that ends a basic block with a `Call` terminator. The
/// call has a void return type; the destination temp is created here. The
/// successor block is `after_bb` — the assertion-failure helpers themselves
/// abort the process, but the CFG must still be well-formed.
fn emit_call_no_return(
    ctx: &mut LoweringContext,
    span: Span,
    runtime_fn: &str,
    args: Vec<Operand>,
    after_bb: crate::mir::BasicBlock,
) {
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let func_op = identifier_constant(runtime_fn, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            out_args: Vec::new(),
            destination: Place::new(void_temp),
            target: Some(after_bb),
        },
        span,
    ));
}

/// Create a `Constant` operand that names the runtime function symbol.
fn identifier_constant(name: &str, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(name.to_string()),
    }))
}

/// Create a `Constant` operand carrying a String literal value.
fn string_literal_operand(ctx: &mut LoweringContext, span: Span, value: String) -> Operand {
    let ty = Type::new(TypeKind::String, span);
    let temp = ctx.push_temp(ty.clone(), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(temp),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty,
                literal: Literal::String(value),
            }))),
        ),
        span,
    });
    Operand::Copy(Place::new(temp))
}

/// Materialise a `void`-typed return value for the assert intrinsics. The
/// callers ignore the result, but call_expr lowering expects every expression
/// path to return an `Operand`.
fn materialize_void(ctx: &mut LoweringContext, span: Span, dest: Option<Place>) -> Operand {
    if let Some(d) = dest {
        return Operand::Copy(d);
    }
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    Operand::Copy(Place::new(void_temp))
}

/// Returns true if `kind` is one of the primitive scalar kinds that the
/// runtime assertion helpers know how to display. The set mirrors the
/// branches handled in `emit_equality` / `emit_to_string`.
fn kind_is_primitive(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Boolean
            | TypeKind::Int
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
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
    )
}

/// Pick the most specific `TypeKind` to drive the equality + display path of
/// `assert_eq`/`assert_ne`. Operand-derived kinds win over the AST-side
/// resolution: the type checker can fail to propagate a function's return
/// type back onto the binding (`let r = call_returning_float()`), but the
/// operand's underlying local was already typed during lowering. Constants
/// carry their declared `ty` directly. Falls back to `fallback` only when
/// neither operand offers a usable kind.
fn pick_concrete_kind(
    ctx: &LoweringContext<'_>,
    actual_op: &Operand,
    expected_op: &Operand,
    fallback: TypeKind,
) -> TypeKind {
    fn kind_of(ctx: &LoweringContext<'_>, op: &Operand) -> Option<TypeKind> {
        match op {
            Operand::Copy(place) | Operand::Move(place) => {
                // A projected place (`t.0`, `s.field`, `xs[i]`) has a value
                // type distinct from the base local — `local_decls` doesn't
                // record that, so leave this operand out of the vote and let
                // the AST-side fallback win.
                if !place.projection.is_empty() {
                    return None;
                }
                let local_kind = &ctx.body.local_decls[place.local.0].ty.kind;
                if matches!(local_kind, TypeKind::Error) {
                    None
                } else {
                    Some(local_kind.clone())
                }
            }
            Operand::Constant(c) if !matches!(c.ty.kind, TypeKind::Error) => {
                Some(c.ty.kind.clone())
            }
            Operand::Constant(_) => None,
        }
    }

    if let Some(kind) = kind_of(ctx, actual_op) {
        return kind;
    }
    if let Some(kind) = kind_of(ctx, expected_op) {
        return kind;
    }
    fallback
}

/// Resolve the monomorphised T for `assert_eq<T>` / `assert_ne<T>`. Reads the
/// type-checker's generic mapping on the call expression, then falls back to
/// the argument's resolved type if the mapping is missing.
fn resolve_value_kind(
    ctx: &LoweringContext<'_>,
    call_expr: &Expression,
    actual_arg: &Expression,
    expected_arg: &Expression,
    span: Span,
) -> Result<TypeKind, LoweringError> {
    if let Some(args) = ctx.type_checker.call_generic_mappings.get(&call_expr.id) {
        if let Some((_, ty)) = args.iter().find(|(name, _)| name == "T") {
            return Ok(ty.kind.clone());
        }
    }

    if let Some(ty) = ctx.type_checker.get_type(actual_arg.id) {
        return Ok(ty.kind.clone());
    }

    if let Some(ty) = ctx.type_checker.get_type(expected_arg.id) {
        return Ok(ty.kind.clone());
    }

    Err(LoweringError::unsupported_expression(
        "assert_eq/assert_ne: cannot resolve the type of compared values".to_string(),
        span,
    ))
}

/// Emit an equality test producing a `bool` local. `String` comparison goes
/// through the runtime `miri_rt_string_equals` helper; everything else uses
/// the direct `Rvalue::BinaryOp(BinOp::Eq, ...)` path that the codegen
/// already implements for primitive types.
fn emit_equality(
    ctx: &mut LoweringContext,
    span: Span,
    kind: &TypeKind,
    a: Operand,
    b: Operand,
) -> Result<Local, LoweringError> {
    let bool_ty = Type::new(TypeKind::Boolean, span);
    match kind {
        TypeKind::String => {
            let result = ctx.push_temp(bool_ty, span);
            let next = ctx.new_basic_block();
            ctx.set_terminator(Terminator::new(
                TerminatorKind::Call {
                    func: identifier_constant(rt::STRING_EQUALS, span),
                    args: vec![a, b],
                    out_args: Vec::new(),
                    destination: Place::new(result),
                    target: Some(next),
                },
                span,
            ));
            ctx.set_current_block(next);
            Ok(result)
        }
        TypeKind::Boolean
        | TypeKind::Int
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
        | TypeKind::F32
        | TypeKind::F64 => {
            let result = ctx.push_temp(bool_ty, span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(result),
                    Rvalue::BinaryOp(BinOp::Eq, Box::new(a), Box::new(b)),
                ),
                span,
            });
            Ok(result)
        }
        other => Err(LoweringError::unsupported_expression(
            format!(
                "assert_eq/assert_ne does not yet support values of type '{}'",
                other
            ),
            span,
        )),
    }
}

/// Convert a value to its display string, with string values wrapped in
/// surrounding `"..."` quotes so the failure message disambiguates them from
/// numeric values.
fn emit_value_to_quoted_string(
    ctx: &mut LoweringContext,
    kind: &TypeKind,
    operand: Operand,
    span: Span,
) -> Result<Local, LoweringError> {
    let value_local = emit_to_string(ctx, operand, kind, &span)?;

    if !matches!(kind, TypeKind::String) {
        return Ok(value_local);
    }

    // Wrap the String in quotes: build `"\"" + value + "\""`.
    let quote_lhs = ctx.push_temp(Type::new(TypeKind::String, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(quote_lhs),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::String, span),
                literal: Literal::String("\"".to_string()),
            }))),
        ),
        span,
    });

    let prefixed = emit_string_concat(ctx, span, quote_lhs, value_local)?;

    let quote_rhs = ctx.push_temp(Type::new(TypeKind::String, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(quote_rhs),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::String, span),
                literal: Literal::String("\"".to_string()),
            }))),
        ),
        span,
    });

    emit_string_concat(ctx, span, prefixed, quote_rhs)
}

/// Emit a `String_concat(a, b, allocator)` call (the stdlib wrapper around
/// `rt::STRING_CONCAT`) and return the resulting Local. We route through the
/// wrapper so the call ABI — including the implicit allocator parameter —
/// stays in sync with the f-string lowering, which uses the same symbol.
fn emit_string_concat(
    ctx: &mut LoweringContext,
    span: Span,
    a: Local,
    b: Local,
) -> Result<Local, LoweringError> {
    let result = ctx.push_temp(Type::new(TypeKind::String, span), span);
    let next = ctx.new_basic_block();
    let mut call_args = vec![Operand::Copy(Place::new(a)), Operand::Copy(Place::new(b))];
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }
    let func_op = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier("String_concat".to_string()),
    }));
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args,
            out_args: Vec::new(),
            destination: Place::new(result),
            target: Some(next),
        },
        span,
    ));
    ctx.set_current_block(next);
    Ok(result)
}
