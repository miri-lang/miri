// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Patterns nested inside a variant's payload: `Some(Shape.Circle(s))`,
//! `Result.Ok(Some(0))`, `Wrap(Shape.Square(n))`.
//!
//! A match dispatches on its subject's own discriminant only. A payload
//! position holding a variant or a literal is a further test the arm must pass,
//! so it is lowered like a guard: the payload is read into a local of its own,
//! its discriminant or value is tested, and a mismatch leaves for the arm a
//! failing guard would leave for. Each level is tested before the level inside
//! it is read, so a payload is only ever read at the variant it was built as.

use crate::ast::literal::Literal;
use crate::ast::pattern::Pattern;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BasicBlock, Discriminant, Local, Operand, Place, PlaceElem, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::{
    bind_pattern, is_option_some_pattern, literal_to_u128, resolve_type, variant_payload_types,
};

/// Whether `pattern`, written in a payload position, can fail to match.
fn is_refutable(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Literal(_)
        | Pattern::Member(..)
        | Pattern::EnumVariant(..)
        | Pattern::Regex(_) => true,
        Pattern::Tuple(elements) => elements.iter().any(is_refutable),
        Pattern::Identifier(_) | Pattern::Default => false,
    }
}

/// Whether any payload position of `pattern` holds a pattern that can fail.
pub(crate) fn has_refutable_payload(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::EnumVariant(_, payloads) | Pattern::Tuple(payloads) => {
            payloads.iter().any(is_refutable)
        }
        Pattern::Literal(_)
        | Pattern::Member(..)
        | Pattern::Regex(_)
        | Pattern::Identifier(_)
        | Pattern::Default => false,
    }
}

/// The index of `variant` among the variants the enum `type_name` declares,
/// which is the discriminant a value built as that variant carries.
pub(crate) fn variant_discriminant(
    ctx: &LoweringContext,
    type_name: &str,
    variant: &str,
) -> Option<u128> {
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
        .position(|(name, _)| name == variant)
        .map(|idx| idx as u128)
}

/// Where an arm goes when a pattern nested in its payload does not match: the
/// block to continue at, and the arm's own scope, whose bindings and payload
/// reads are released on the way out as a `break` releases a loop body's.
#[derive(Clone, Copy)]
pub(crate) struct ArmExit {
    pub block: BasicBlock,
    pub scope: usize,
}

/// Bind `pattern` against `subject`, testing every refutable pattern nested
/// in its payload on the way and leaving through `fail` at the first one that
/// does not match. The current block is left where every test has passed.
pub(crate) fn bind_testing_payloads(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    subject: Local,
    span: Span,
    fail: ArmExit,
) -> Result<(), LoweringError> {
    bind_pattern(ctx, pattern, subject, &span)?;
    match pattern {
        Pattern::EnumVariant(parent, payloads) => {
            bind_testing_variant_payloads(ctx, parent, payloads, subject, span, fail)
        }
        Pattern::Tuple(elements) => bind_testing_tuple_elements(ctx, elements, subject, span, fail),
        Pattern::Literal(_)
        | Pattern::Member(..)
        | Pattern::Regex(_)
        | Pattern::Identifier(_)
        | Pattern::Default => Ok(()),
    }
}

/// Test and bind each refutable element of a tuple pattern, read from the
/// element's own field of `subject`.
fn bind_testing_tuple_elements(
    ctx: &mut LoweringContext,
    elements: &[Pattern],
    subject: Local,
    span: Span,
    fail: ArmExit,
) -> Result<(), LoweringError> {
    let TypeKind::Tuple(element_types) = ctx.body.local_decls[subject.0].ty.kind.clone() else {
        return Err(unsupported_nesting(span));
    };
    for (position, element) in elements.iter().enumerate() {
        if !is_refutable(element) {
            continue;
        }
        let element_ty = element_types
            .get(position)
            .map(|ty| resolve_type(ctx.type_checker, ty))
            .ok_or_else(|| unsupported_nesting(span))?;
        let element_local = read_payload(ctx, subject, position, element_ty, span);
        test_payload(ctx, element, element_local, span, fail)?;
        bind_testing_payloads(ctx, element, element_local, span, fail)?;
    }
    Ok(())
}

/// Test and bind each refutable payload of a variant pattern whose variant is
/// written `parent`.
fn bind_testing_variant_payloads(
    ctx: &mut LoweringContext,
    parent: &Pattern,
    payloads: &[Pattern],
    subject: Local,
    span: Span,
    fail: ArmExit,
) -> Result<(), LoweringError> {
    let subject_ty = ctx.body.local_decls[subject.0].ty.clone();
    let is_some = is_option_some_pattern(&subject_ty, parent);
    let payload_types = if is_some {
        None
    } else {
        variant_payload_types(ctx, parent, subject)
    };
    for (position, payload) in payloads.iter().enumerate() {
        if !is_refutable(payload) {
            continue;
        }
        let (field, payload_ty) = if is_some {
            let TypeKind::Option(inner) = &subject_ty.kind else {
                return Err(unsupported_nesting(span));
            };
            (0, inner.as_ref().clone())
        } else {
            let payload_ty = payload_types
                .as_ref()
                .and_then(|types| types.get(position))
                .cloned()
                .ok_or_else(|| unsupported_nesting(span))?;
            (position + 1, payload_ty)
        };
        let payload_local = read_payload(ctx, subject, field, payload_ty, span);
        test_payload(ctx, payload, payload_local, span, fail)?;
        bind_testing_payloads(ctx, payload, payload_local, span, fail)?;
    }
    Ok(())
}

/// Read field `field` of `subject` into a local of its own, owned by the arm's
/// scope the way a bound name is, so a managed payload is retained for the
/// reads below and released with the arm.
fn read_payload(
    ctx: &mut LoweringContext,
    subject: Local,
    field: usize,
    payload_ty: Type,
    span: Span,
) -> Local {
    // A name no source can write, unique per local so each one is released.
    let name = format!("$payload{}", ctx.body.local_decls.len());
    let local = ctx.push_local(name, payload_ty, span);
    let mut place = Place::new(subject);
    place.projection.push(PlaceElem::Field(field));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(local), Rvalue::Use(Operand::Copy(place))),
        span,
    });
    local
}

/// Branch to `fail` unless the value in `payload` matches `pattern`'s own
/// outermost variant or literal; continue in a fresh block when it does.
fn test_payload(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    payload: Local,
    span: Span,
    exit: ArmExit,
) -> Result<(), LoweringError> {
    let payload_ty = ctx.body.local_decls[payload.0].ty.clone();
    let (discr, expected) = match pattern {
        // An optional's own value is its discriminant: zero is `None`.
        Pattern::Literal(Literal::None) => (Operand::Copy(Place::new(payload)), Expected::Is(0)),
        Pattern::EnumVariant(parent, _) if is_option_some_pattern(&payload_ty, parent) => {
            (Operand::Copy(Place::new(payload)), Expected::IsNot(0))
        }
        Pattern::Literal(literal) => {
            let value = literal_to_u128(literal).ok_or_else(|| unsupported_nesting(span))?;
            (Operand::Copy(Place::new(payload)), Expected::Is(value))
        }
        Pattern::Member(type_pattern, variant) => {
            let index = enum_variant_index(ctx, type_pattern, variant, span)?;
            (read_discriminant(ctx, payload, span), Expected::Is(index))
        }
        Pattern::EnumVariant(parent, _) => {
            let Pattern::Member(type_pattern, variant) = parent.as_ref() else {
                return Err(unsupported_nesting(span));
            };
            let index = enum_variant_index(ctx, type_pattern, variant, span)?;
            (read_discriminant(ctx, payload, span), Expected::Is(index))
        }
        // A tuple always has its shape; its elements are tested one by one
        // as the pattern is bound.
        Pattern::Tuple(_) => return Ok(()),
        Pattern::Regex(_) | Pattern::Identifier(_) | Pattern::Default => {
            return Err(unsupported_nesting(span));
        }
    };
    let pass = ctx.new_basic_block();
    let fail = ctx.new_basic_block();
    let (target, otherwise) = match expected {
        Expected::Is(value) => ((value, pass), fail),
        Expected::IsNot(value) => ((value, fail), pass),
    };
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr,
            targets: vec![(Discriminant::from(target.0), target.1)],
            otherwise,
        },
        span,
    ));
    leave_arm(ctx, fail, exit, span);
    ctx.set_current_block(pass);
    Ok(())
}

/// Fill `from` with the release of everything the arm's scope holds so far,
/// then the jump to where the arm's exit leads.
fn leave_arm(ctx: &mut LoweringContext, from: BasicBlock, exit: ArmExit, span: Span) {
    ctx.set_current_block(from);
    ctx.emit_break_cleanup(exit.scope, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: exit.block },
        span,
    ));
}

/// What a payload's discriminant must be for the nested pattern to match.
enum Expected {
    Is(u128),
    IsNot(u128),
}

fn enum_variant_index(
    ctx: &LoweringContext,
    type_pattern: &Pattern,
    variant: &str,
    span: Span,
) -> Result<u128, LoweringError> {
    let Pattern::Identifier(type_name) = type_pattern else {
        return Err(unsupported_nesting(span));
    };
    variant_discriminant(ctx, type_name, variant).ok_or_else(|| unsupported_nesting(span))
}

/// Read the discriminant an enum value carries in its first field.
fn read_discriminant(ctx: &mut LoweringContext, value: Local, span: Span) -> Operand {
    let discr_local = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    let mut place = Place::new(value);
    place.projection.push(PlaceElem::Field(0));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(discr_local), Rvalue::Use(Operand::Copy(place))),
        span,
    });
    Operand::Copy(Place::new(discr_local))
}

/// A nested pattern this lowering cannot test — a string, float or regex
/// literal in a payload position or a tuple element.
/// Refused rather than bound unchecked, which would take the arm whatever the
/// payload holds.
fn unsupported_nesting(span: Span) -> LoweringError {
    // TODO: string, float and regex literals nested in a payload or a tuple
    // element need a predicate test here, the way a top-level one is tested
    // in `emit_predicate_test_chain`.
    LoweringError::unsupported_expression(
        "this pattern nested inside another pattern cannot be matched yet: \
         only variants and integer or boolean literals can be nested",
        span,
    )
}
