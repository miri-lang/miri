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
use crate::mir::lowering::expression::match_expr::{
    emit_regex_predicate_test, emit_simple_predicate_test,
};
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

/// Whether an arm whose patterns are `alternatives` has to find out which one
/// matched: one of them tests a nested pattern, which can fail after the arm
/// was entered, or binds a name, which each alternative reads from a place of
/// its own.
pub(crate) fn alternatives_need_tests(alternatives: &[Pattern]) -> bool {
    alternatives.iter().any(|alternative| {
        has_refutable_payload(alternative) || !bound_names(alternative).is_empty()
    })
}

/// Bind the first of `alternatives` that `subject` matches, or leave for
/// `fallthrough` when none does.
///
/// Each alternative is tested in a scope of its own: its outermost shape, then
/// every pattern nested in it, a failure moving on to the next alternative.
/// The one that matches hands each name it binds to a slot the alternatives
/// share, then releases its scope; the arm binds every name from its slot once
/// any alternative has matched, so the body reads one local per name whichever
/// alternative bound it.
pub(crate) fn bind_matching_alternative(
    ctx: &mut LoweringContext,
    alternatives: &[Pattern],
    subject: Local,
    span: Span,
    fallthrough: BasicBlock,
) -> Result<(), LoweringError> {
    let matched = ctx.new_basic_block();
    let mut slots: Vec<BindingSlot> = Vec::new();
    for (index, alternative) in alternatives.iter().enumerate() {
        let next = if index + 1 == alternatives.len() {
            fallthrough
        } else {
            ctx.new_basic_block()
        };
        ctx.push_scope();
        let exit = ArmExit {
            block: next,
            scope: ctx.scope_depth(),
        };
        test_payload(ctx, alternative, subject, span, exit)?;
        bind_testing_payloads(ctx, alternative, subject, span, exit)?;
        fill_binding_slots(ctx, alternative, &mut slots, index == 0, span)?;
        ctx.pop_scope(span);
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: matched },
            span,
        ));
        ctx.set_current_block(next);
    }
    ctx.set_current_block(matched);
    for slot in slots {
        let ty = ctx.body.local_decls[slot.local.0].ty.clone();
        let local = ctx.push_local(slot.name, ty, span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(local),
                Rvalue::Use(Operand::Move(Place::new(slot.local))),
            ),
            span,
        });
    }
    Ok(())
}

/// A name an arm's alternatives bind, and the temp each alternative leaves
/// its value in. The temp holds the reference the arm's own binding takes
/// over, so nothing else releases it.
struct BindingSlot {
    name: String,
    local: Local,
}

/// Copy each name `alternative` has just bound into its slot, creating the
/// slots from the first alternative. Every later alternative must bind the
/// same names, which the type checker requires of an arm.
fn fill_binding_slots(
    ctx: &mut LoweringContext,
    alternative: &Pattern,
    slots: &mut Vec<BindingSlot>,
    is_first: bool,
    span: Span,
) -> Result<(), LoweringError> {
    let mut names = bound_names(alternative);
    names.sort_unstable();
    names.dedup();
    if is_first {
        for name in names {
            let bound = bound_local(ctx, name, span)?;
            let ty = ctx.body.local_decls[bound.0].ty.clone();
            let local = ctx.push_temp(ty, span);
            slots.push(BindingSlot {
                name: name.to_string(),
                local,
            });
        }
    } else if names.len() != slots.len()
        || slots
            .iter()
            .any(|slot| !names.contains(&slot.name.as_str()))
    {
        return Err(LoweringError::unsupported_expression(
            "the alternatives of this arm bind different names",
            span,
        ));
    }
    for slot in slots.iter() {
        let bound = bound_local(ctx, &slot.name, span)?;
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(slot.local),
                Rvalue::Use(Operand::Copy(Place::new(bound))),
            ),
            span,
        });
    }
    Ok(())
}

/// The local `name` was just bound to.
fn bound_local(ctx: &LoweringContext, name: &str, span: Span) -> Result<Local, LoweringError> {
    ctx.variable_map.get(name).copied().ok_or_else(|| {
        LoweringError::unsupported_expression(
            format!("the name '{name}' an alternative of this arm binds was not bound"),
            span,
        )
    })
}

/// Every name `pattern` binds, at any depth.
fn bound_names(pattern: &Pattern) -> Vec<&str> {
    match pattern {
        Pattern::Identifier(name) if name != "_" => vec![name.as_str()],
        Pattern::EnumVariant(_, payloads) | Pattern::Tuple(payloads) => {
            payloads.iter().flat_map(bound_names).collect()
        }
        Pattern::Identifier(_)
        | Pattern::Literal(_)
        | Pattern::Member(..)
        | Pattern::Regex(_)
        | Pattern::Default => Vec::new(),
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

/// Branch to `exit` unless the value in `payload` matches `pattern`'s own
/// outermost variant or literal; continue in a fresh block when it does.
fn test_payload(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    payload: Local,
    span: Span,
    exit: ArmExit,
) -> Result<(), LoweringError> {
    let test = payload_test(ctx, pattern, payload, span)?;
    if matches!(test, PayloadTest::Irrefutable) {
        return Ok(());
    }
    let pass = ctx.new_basic_block();
    let fail = ctx.new_basic_block();
    match test {
        PayloadTest::Irrefutable => {}
        PayloadTest::Discriminant(discr, expected) => {
            branch_on_discriminant(ctx, discr, expected, (pass, fail), span);
        }
        PayloadTest::Equals => {
            let is_equal = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
            emit_simple_predicate_test(ctx, pattern, payload, &span, is_equal)?;
            let discr = Operand::Copy(Place::new(is_equal));
            branch_on_discriminant(ctx, discr, Expected::Is(1), (pass, fail), span);
        }
        PayloadTest::Regex => {
            emit_regex_predicate_test(ctx, pattern, payload, &span, pass, fail)?;
        }
    }
    leave_arm(ctx, fail, exit, span);
    ctx.set_current_block(pass);
    Ok(())
}

/// How a value is tested against one pattern's outermost shape.
enum PayloadTest {
    /// The pattern matches whatever the value holds.
    Irrefutable,
    /// The operand must hold, or must not hold, a given value.
    Discriminant(Operand, Expected),
    /// The value must equal a string or float literal.
    Equals,
    /// The value must match a regex literal.
    Regex,
}

/// Decide how `payload` is tested against `pattern`, emitting the read of an
/// enum's discriminant when the test needs one.
fn payload_test(
    ctx: &mut LoweringContext,
    pattern: &Pattern,
    payload: Local,
    span: Span,
) -> Result<PayloadTest, LoweringError> {
    let payload_ty = ctx.body.local_decls[payload.0].ty.clone();
    let own_value = || Operand::Copy(Place::new(payload));
    let test = match pattern {
        // An optional's own value is its discriminant: zero is `None`.
        Pattern::Literal(Literal::None) => PayloadTest::Discriminant(own_value(), Expected::Is(0)),
        Pattern::Member(..) if is_option_none_pattern(&payload_ty, pattern) => {
            PayloadTest::Discriminant(own_value(), Expected::Is(0))
        }
        Pattern::EnumVariant(parent, _) if is_option_some_pattern(&payload_ty, parent) => {
            PayloadTest::Discriminant(own_value(), Expected::IsNot(0))
        }
        Pattern::Literal(Literal::String(_) | Literal::Float(_)) => PayloadTest::Equals,
        Pattern::Literal(literal) => {
            let value = literal_to_u128(literal).ok_or_else(|| unsupported_nesting(span))?;
            PayloadTest::Discriminant(own_value(), Expected::Is(value))
        }
        Pattern::Member(type_pattern, variant) => {
            let index = enum_variant_index(ctx, type_pattern, variant, &payload_ty, span)?;
            PayloadTest::Discriminant(read_discriminant(ctx, payload, span), Expected::Is(index))
        }
        Pattern::EnumVariant(parent, _) => {
            let Pattern::Member(type_pattern, variant) = parent.as_ref() else {
                return Err(unsupported_nesting(span));
            };
            let index = enum_variant_index(ctx, type_pattern, variant, &payload_ty, span)?;
            PayloadTest::Discriminant(read_discriminant(ctx, payload, span), Expected::Is(index))
        }
        Pattern::Regex(_) => PayloadTest::Regex,
        // A tuple always has its shape; its elements are tested one by one
        // as the pattern is bound.
        Pattern::Tuple(_) | Pattern::Identifier(_) | Pattern::Default => PayloadTest::Irrefutable,
    };
    Ok(test)
}

/// Whether `pattern` is `Option.None` written against an optional value.
fn is_option_none_pattern(value_ty: &Type, pattern: &Pattern) -> bool {
    let Pattern::Member(type_pattern, variant) = pattern else {
        return false;
    };
    matches!(value_ty.kind, TypeKind::Option(_))
        && variant == "None"
        && matches!(
            type_pattern.as_ref(),
            Pattern::Identifier(name) if name == crate::ast::types::OPTION_TYPE_NAME
        )
}

/// End the current block with a branch to `pass` when `discr` meets
/// `expected`, and to `fail` otherwise.
fn branch_on_discriminant(
    ctx: &mut LoweringContext,
    discr: Operand,
    expected: Expected,
    (pass, fail): (BasicBlock, BasicBlock),
    span: Span,
) {
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
}

/// Fill `from` with the release of everything the arm's scope holds so far,
/// then the jump to where the arm's exit leads.
pub(crate) fn leave_arm(ctx: &mut LoweringContext, from: BasicBlock, exit: ArmExit, span: Span) {
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
    payload_ty: &Type,
    span: Span,
) -> Result<u128, LoweringError> {
    let Pattern::Identifier(_) = type_pattern else {
        return Err(unsupported_nesting(span));
    };
    crate::mir::lowering::helpers::pattern_enum_identity(&payload_ty.kind)
        .and_then(|enum_name| variant_discriminant(ctx, enum_name, variant))
        .ok_or_else(|| unsupported_nesting(span))
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

/// A nested pattern whose test cannot be resolved: a variant of a type that
/// is not an enum the type table knows, or a payload position the variant
/// does not have. The type checker refuses both first, so reaching this is a
/// lowering bug, reported rather than bound unchecked.
fn unsupported_nesting(span: Span) -> LoweringError {
    LoweringError::unsupported_expression(
        "this pattern nested inside another pattern names no variant or payload \
         the matched value has",
        span,
    )
}
