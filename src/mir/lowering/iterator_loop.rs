// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `for x in c` over a value walked through its iterator: an `Iterable` that
//! is not `Indexed`.
//!
//! ```text
//! c  = <iterable>                   loop-scope temp
//! it = iterator(c)                  loop-scope temp, released at exit and on early exits
//! header: o = next(it); switchInt(o) [None -> exit, otherwise -> body]
//! body:   x = (o as Some).0         o and x released by the pass's scope
//!         [v? = value(it); None -> leave; v = (v? as Some).0]
//! ```
//!
//! The optional `next` returns is switched on and read as an `if let Some`
//! reads one, so the optional layout's own rules decide how it is held. Each
//! method is called as a written call is: `iterator()` on the iterable, so a
//! subclass's override is reached, and `next` and `value` on the iterator as
//! the type checker's settled plan holds it — the class it proved every
//! `iterator()` the loop can reach returns, or the `Iterator` trait.

use crate::ast::expression::Expression;
use crate::ast::statement::Statement;
use crate::ast::types::{
    Type, TypeKind, ENTRY_VALUE_METHOD_NAME, ITERATOR_METHOD_NAME, NEXT_METHOD_NAME,
};
use crate::ast::VariableDeclaration;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    BasicBlock, Discriminant, Local, Operand, Place, PlaceElem, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use crate::type_checker::ProtocolWalk;

use super::loops::{
    assign, bind_loop_variables, goto, lower_loop_iterable, new_latch_block, LoopBindings,
};
use super::method_dispatch::{emit_held_receiver_method_call, HeldReceiverCall};
use super::{lower_statement, LoweringContext};

/// A `for` loop over a value its plan walks through an iterator.
pub(super) struct IteratorLoop<'a> {
    pub decls: &'a [VariableDeclaration],
    pub iterable: &'a Expression,
    pub walk: ProtocolWalk,
    pub body: &'a Statement,
}

/// What each pass of an iterator loop reads from: the iterator, and the
/// optional its `next` returns into.
struct IteratorPass<'a> {
    walk: &'a ProtocolWalk,
    iterator: Local,
    element: Local,
}

/// Lower `for x in c`, or `for x, v in c`, through `c`'s iterator.
pub(super) fn lower_for_through_iterator(
    ctx: &mut LoweringContext,
    lowered: IteratorLoop<'_>,
    span: &Span,
) -> Result<(), LoweringError> {
    let mut lowered = lowered;
    lowered.walk = instantiated_walk(ctx, lowered.walk);
    ctx.push_scope();
    let iterable_ty = ctx.recorded_type(lowered.iterable.id);
    let second_ty = second_variable_type(&lowered, span)?;
    let bindings = bind_loop_variables(ctx, span, lowered.decls, &iterable_ty, second_ty);
    let source = lower_loop_iterable(ctx, lowered.iterable, iterable_ty.clone(), span)?;
    let iterator = start_iterator(ctx, &lowered.walk, source, iterable_ty, span)?;

    let header = ctx.new_basic_block();
    let body_bb = ctx.new_basic_block();
    let exhausted = ctx.new_basic_block();
    let exit = ctx.new_basic_block();
    goto(ctx, header, span);
    let latch = new_latch_block(ctx, header, *span);

    ctx.set_current_block(header);
    let element_ty = ctx.body.local_decls[bindings.element.0].ty.clone();
    let element = call_iterator_method(
        ctx,
        &lowered.walk,
        iterator,
        (NEXT_METHOD_NAME, element_ty),
        span,
    )?;
    switch_on_presence(ctx, element, (body_bb, exhausted), span);
    ctx.set_current_block(exhausted);
    release_temp(ctx, element, span);
    goto(ctx, exit, span);

    let pass = IteratorPass {
        walk: &lowered.walk,
        iterator,
        element,
    };
    ctx.enter_loop(exit, latch);
    ctx.set_current_block(body_bb);
    lower_iterator_pass(ctx, &bindings, &pass, lowered.body, span)?;
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        goto(ctx, latch, span);
    }
    ctx.exit_loop();

    ctx.set_current_block(exit);
    for name in &bindings.per_pass_names {
        ctx.variable_map.remove(name.as_str());
    }
    ctx.pop_scope(*span);
    Ok(())
}

/// `walk` with the types it names read at the active instantiation: the
/// checker recorded them once, in a generic body's own parameters.
fn instantiated_walk(ctx: &LoweringContext, walk: ProtocolWalk) -> ProtocolWalk {
    let instantiate =
        |ty: &Type| super::apply_generic_sub(&ctx.resolve_self_in(ty), &ctx.generic_subs);
    ProtocolWalk {
        iterator: instantiate(&walk.iterator),
        entries: walk.entries.as_ref().map(instantiate),
        ..walk
    }
}

/// The type of the loop's second variable, the iterator's `Entries` value,
/// when the loop names one.
fn second_variable_type(
    lowered: &IteratorLoop<'_>,
    span: &Span,
) -> Result<Option<Type>, LoweringError> {
    if lowered.decls.len() < 2 {
        return Ok(None);
    }
    match &lowered.walk.entries {
        Some(value) => Ok(Some(value.clone())),
        None => Err(LoweringError::internal(
            crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
            format!(
                "a two-variable loop over an iterator `{}` with no `Entries` value reached \
                 lowering",
                lowered.walk.iterator
            ),
            *span,
        )),
    }
}

/// Call the iterable's `iterator()` on `source` into a temp the loop's scope
/// owns, so the iterator is released when the loop ends however it ends. The
/// call is dispatched as a written `c.iterator()` is, reaching a subclass's
/// override.
fn start_iterator(
    ctx: &mut LoweringContext,
    walk: &ProtocolWalk,
    source: Local,
    iterable_ty: Option<Type>,
    span: &Span,
) -> Result<Local, LoweringError> {
    let receiver_ty = iterable_ty.ok_or_else(|| {
        LoweringError::internal(
            crate::diagnostics::DiagnosticCode::CgInternalCodegenError,
            "a loop walking an iterator reached lowering with no recorded iterable type"
                .to_string(),
            *span,
        )
    })?;
    let iterator = ctx.push_temp(walk.iterator.clone(), *span);
    ctx.register_scope_temp(iterator);
    let call = HeldReceiverCall {
        receiver_ty: &receiver_ty,
        method_name: ITERATOR_METHOD_NAME,
        args: vec![Operand::Copy(Place::new(source))],
    };
    emit_held_receiver_method_call(ctx, call, &Place::new(iterator), *span)?;
    Ok(iterator)
}

/// Call `method` on the iterator into a new temp holding an optional of
/// `payload`, dispatched as a written call on the iterator is.
fn call_iterator_method(
    ctx: &mut LoweringContext,
    walk: &ProtocolWalk,
    iterator: Local,
    (method, payload): (&str, Type),
    span: &Span,
) -> Result<Local, LoweringError> {
    let optional = Type::new(TypeKind::Option(Box::new(payload)), *span);
    let result = ctx.push_temp(optional, *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::StorageLive(Place::new(result)),
        span: *span,
    });
    let call = HeldReceiverCall {
        receiver_ty: &walk.iterator,
        method_name: method,
        args: vec![Operand::Copy(Place::new(iterator))],
    };
    emit_held_receiver_method_call(ctx, call, &Place::new(result), *span)?;
    Ok(result)
}

/// Go to `present` when the optional in `optional` holds a value and to
/// `absent` when it is `None`.
fn switch_on_presence(
    ctx: &mut LoweringContext,
    optional: Local,
    (present, absent): (BasicBlock, BasicBlock),
    span: &Span,
) {
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(optional)),
            targets: vec![(Discriminant::from(0u128), absent)],
            otherwise: present,
        },
        *span,
    ));
}

/// End `temp`'s life, which releases it when its type is counted.
fn release_temp(ctx: &mut LoweringContext, temp: Local, span: &Span) {
    let kind = ctx.body.local_decls[temp.0].ty.kind.clone();
    if ctx.is_counted(&kind) {
        ctx.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageDead(Place::new(temp)),
            span: *span,
        });
    }
}

/// Lower one pass: bind the element `next` returned and, when declared, the
/// iterator's `Entries` value, then run the body.
///
/// The pass opens a scope of its own that owns the optionals `next` and
/// `value` returned and every managed binding, so falling off the end of the
/// body, `break`, `continue` and `return` each release them exactly once. A
/// `value()` of `None` ends the loop the way `break` does.
fn lower_iterator_pass(
    ctx: &mut LoweringContext,
    bindings: &LoopBindings,
    pass: &IteratorPass<'_>,
    body: &Statement,
    span: &Span,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    ctx.register_scope_temp(pass.element);
    let is_second = |local: &&Local| bindings.secondary == Some(**local);
    for &local in bindings.per_pass.iter().filter(|local| !is_second(local)) {
        hold_per_pass(ctx, local, span);
    }
    assign(
        ctx,
        bindings.element,
        Rvalue::Use(payload_of(pass.element)),
        span,
    );
    if let Some(second) = bindings.secondary {
        let managed = bindings.per_pass.contains(&second);
        bind_entry_value(ctx, pass, (second, managed), span)?;
    }
    lower_statement(ctx, body)?;
    ctx.pop_scope(*span);
    Ok(())
}

/// Start a per-pass binding's life in the pass's scope, which releases it
/// however the pass ends.
fn hold_per_pass(ctx: &mut LoweringContext, local: Local, span: &Span) {
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::StorageLive(Place::new(local)),
        span: *span,
    });
    ctx.register_scope_temp(local);
}

/// Read the iterator's `value()` into the loop's second variable, leaving the
/// loop when it is `None`.
///
/// A managed second variable starts its life only once `value()` is known to
/// hold a value: leaving on `None` must not release it, since it still holds
/// the value the previous pass released, or none at all.
fn bind_entry_value(
    ctx: &mut LoweringContext,
    pass: &IteratorPass<'_>,
    (second, managed): (Local, bool),
    span: &Span,
) -> Result<(), LoweringError> {
    let value_ty = ctx.body.local_decls[second.0].ty.clone();
    let method = (ENTRY_VALUE_METHOD_NAME, value_ty);
    let value = call_iterator_method(ctx, pass.walk, pass.iterator, method, span)?;
    ctx.register_scope_temp(value);
    let present = ctx.new_basic_block();
    let absent = ctx.new_basic_block();
    switch_on_presence(ctx, value, (present, absent), span);
    ctx.set_current_block(absent);
    super::loops::lower_break(ctx, span)?;
    ctx.set_current_block(present);
    if managed {
        hold_per_pass(ctx, second, span);
    }
    assign(ctx, second, Rvalue::Use(payload_of(value)), span);
    Ok(())
}

/// The value an optional held in `optional` carries, read in place.
fn payload_of(optional: Local) -> Operand {
    Operand::Copy(Place {
        local: optional,
        projection: vec![PlaceElem::Field(0)],
    })
}
