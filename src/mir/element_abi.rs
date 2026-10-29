// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The element ABI: how a call hands a collection one of its elements.
//!
//! A call that stores or looks up an element is lowered with the element as
//! one argument, which is how reference counting and verification read it: the
//! element is a value the container takes or borrows. The runtime entry point
//! takes that argument as two — the address of the element's bytes and the
//! number of those bytes that are the element — for every collection and every
//! element width. This pass spells that expansion in MIR, so no backend has to
//! know which arguments are elements or how wide each one is.
//!
//! A call that hands an element back is lowered with the element as its
//! result. The runtime entry point instead writes the element into storage the
//! caller names — again an address and a byte count, appended after its other
//! arguments — and returns nothing, so an element of any width arrives whole
//! and at its own width. This pass spells that too: the destination becomes an
//! `out` argument, or the address of the bytes of the value it is held as.
//!
//! The positions come from [`crate::runtime_fns::element_positions`] and
//! [`crate::runtime_fns::returns_element_value`]; the width from
//! [`crate::type_checker::element_layout::element_layout`], read against the
//! body's own layout scope.
//! The pass runs after reference counting, which has already accounted for the
//! element as the value it is.

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::place_type::PlaceTypes;
use crate::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, Constant, LocalDecl, MirType, Operand, Place,
    Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
pub use crate::type_checker::element_layout::{element_layout, ElementLayout};

/// Rewrite every element argument in `body` into its address and byte count,
/// and every element result into caller storage the runtime writes.
///
/// An element that cannot be sized — a projection reaching a type this pass
/// cannot read — is reported rather than guessed at.
pub fn pass_elements_by_address(body: &mut Body) -> Result<(), String> {
    for block in 0..body.basic_blocks.len() {
        pass_element_arguments(body, block)?;
        return_element_through_storage(body, block)?;
    }
    Ok(())
}

/// Expand each element argument of the call ending `block` into its address
/// and byte count.
fn pass_element_arguments(body: &mut Body, block: usize) -> Result<(), String> {
    let Some((positions, args, span)) = element_call(body, block) else {
        return Ok(());
    };
    let mut expanded = Vec::with_capacity(args.len() + positions.len());
    for (index, arg) in args.into_iter().enumerate() {
        if positions.contains(&index) {
            let (address, payload) = element_address(body, block, arg, span)?;
            expanded.push(address);
            expanded.push(payload);
        } else {
            expanded.push(arg);
        }
    }
    replace_call_args(body, block, expanded, positions);
    Ok(())
}

/// The element positions, arguments and span of the call ending `block`, when
/// it calls a runtime entry point that takes an element.
fn element_call(body: &Body, block: usize) -> Option<(&'static [usize], Vec<Operand>, Span)> {
    let terminator = body.basic_blocks[block].terminator.as_ref()?;
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return None;
    };
    let positions = crate::runtime_fns::element_positions(func.called_runtime_name()?);
    if positions.is_empty() {
        return None;
    }
    Some((positions, args.clone(), terminator.span))
}

/// Install `expanded` as the arguments of the call ending `block`, widening its
/// per-argument metadata to match: each element position gained one argument
/// after it, which is passed borrowed and carries no device handle.
fn replace_call_args(body: &mut Body, block: usize, expanded: Vec<Operand>, positions: &[usize]) {
    let Some(terminator) = body.basic_blocks[block].terminator.as_mut() else {
        return;
    };
    let TerminatorKind::Call {
        args,
        arg_modes,
        arg_handles,
        ..
    } = &mut terminator.kind
    else {
        return;
    };
    for &position in positions.iter().rev() {
        if position < arg_modes.len() {
            arg_modes.insert(position + 1, ArgMode::Borrowed);
        }
        if position < arg_handles.len() {
            arg_handles.insert(position + 1, None);
        }
    }
    *args = expanded;
}

/// The address of the element `arg` and the byte count to read from it,
/// taking the address in `block` when the element is a value.
///
/// A collection method's shared body declares its element at the bare type
/// parameter, whose layout here is a value word. That body is reached only by
/// elements that are one: every other element type gets a body of its own,
/// compiled at the instantiation's element type.
fn element_address(
    body: &mut Body,
    block: usize,
    arg: Operand,
    span: Span,
) -> Result<(Operand, Operand), String> {
    let layout = argument_layout(body, &arg)?;
    let payload = byte_count(layout.payload, span);
    let place = match arg {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Constant(constant) => {
            let temp = body.new_local(LocalDecl::new(constant.ty.clone(), span));
            let value = Rvalue::Use(Operand::Constant(constant));
            push_assign(body, block, Place::new(temp), value, span);
            Place::new(temp)
        }
        Operand::Function(function) => {
            return Err(format!(
                "the function {} cannot be passed as a collection element",
                function.symbol
            ));
        }
    };
    let address = body.new_local(LocalDecl::new(Type::new(TypeKind::RawPtr, span), span));
    push_assign(body, block, Place::new(address), Rvalue::Ref(place), span);
    Ok((Operand::Copy(Place::new(address)), payload))
}

/// The layout of the element an argument carries.
///
/// A local or a constant is sized by its declared type. A projected place — a
/// field or an element read straight into the call — is sized by the type its
/// projection reaches.
fn argument_layout(body: &Body, arg: &Operand) -> Result<ElementLayout, String> {
    match arg {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Ok(element_layout(
                &body.local_decls[place.local.0].ty.kind,
                body.layout_scope(),
            ))
        }
        Operand::Constant(constant) => Ok(element_layout(&constant.ty.kind, body.layout_scope())),
        Operand::Function(function) => Ok(element_layout(&function.ty.kind, body.layout_scope())),
        Operand::Copy(place) | Operand::Move(place) => {
            if let Some(value) = projected_value(body, place) {
                return Ok(element_layout(&value, body.layout_scope()));
            }
            let reached = arg.ty_projected(body).ok_or_else(|| {
                format!("the collection element {place} reaches a type this pass cannot size")
            })?;
            Ok(projected_layout(body, &reached))
        }
    }
}

/// The type of the value held as its bytes that the projected `place`
/// reaches, or `None` when it reaches anything else.
fn projected_value(body: &Body, place: &Place) -> Option<TypeKind> {
    let declared = PlaceTypes::of(body).declared(place)?;
    body.value_layout(&declared.kind)?;
    Some(declared.kind)
}

/// The layout of an element whose type is known only as the MIR type a
/// projection reaches: a scalar, or a value word.
fn projected_layout(body: &Body, reached: &MirType) -> ElementLayout {
    element_layout(
        &register_scalar(reached).unwrap_or(TypeKind::RawPtr),
        body.layout_scope(),
    )
}

/// The scalar type a value of MIR type `reached` is held as in one register,
/// or `None` for anything held as a reference or laid out inline.
fn register_scalar(reached: &MirType) -> Option<TypeKind> {
    let scalar = match reached {
        MirType::I8 => TypeKind::I8,
        MirType::I16 => TypeKind::I16,
        MirType::I32 => TypeKind::I32,
        MirType::I64 => TypeKind::I64,
        MirType::I128 => TypeKind::I128,
        MirType::U8 => TypeKind::U8,
        MirType::U16 => TypeKind::U16,
        MirType::U32 => TypeKind::U32,
        MirType::U64 => TypeKind::U64,
        MirType::U128 => TypeKind::U128,
        MirType::F32 => TypeKind::F32,
        MirType::F16 => TypeKind::F16,
        MirType::F64 => TypeKind::F64,
        MirType::Int => TypeKind::Int,
        MirType::Float => TypeKind::Float,
        MirType::Boolean => TypeKind::Boolean,
        MirType::Void
        | MirType::Identifier
        | MirType::RawPtr
        | MirType::Error
        | MirType::String
        | MirType::List(_)
        | MirType::Array(_)
        | MirType::Map(_, _)
        | MirType::Set(_)
        | MirType::Tuple(_)
        | MirType::Result(_, _)
        | MirType::Option(_)
        | MirType::Future(_)
        | MirType::Custom(_)
        | MirType::Function
        | MirType::Generic
        | MirType::Unknown => return None,
    };
    Some(scalar)
}

/// Append `place = rvalue` to the statements of `block`, ahead of its call.
fn push_assign(body: &mut Body, block: usize, place: Place, rvalue: Rvalue, span: Span) {
    body.basic_blocks[block].statements.push(Statement {
        kind: StatementKind::Assign(place, rvalue),
        span,
    });
}

/// The pointer-sized count of an element's bytes, as the call passes it.
fn byte_count(bytes: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(bytes)),
    }))
}

/// Route the element the call ending `block` hands back into storage the
/// runtime writes, when the call is to an entry point that hands one back.
///
/// The call gains two trailing arguments — where the element goes and how many
/// bytes it is there — and its result becomes nothing. A destination held in
/// one register is itself the `out` argument. A value held as its bytes hands
/// over the address of those bytes. Anything else — a reference, or a
/// destination reached through a projection — is written into a fresh
/// register-sized slot that the destination is assigned from once the call
/// returns.
fn return_element_through_storage(body: &mut Body, block: usize) -> Result<(), String> {
    let Some((destination, target, span)) = element_returning_call(body, block) else {
        return Ok(());
    };
    let kind = destination_kind(body, &destination)?;
    let layout = element_layout(&kind, body.layout_scope());
    let (storage, is_out) = if body.value_layout(&kind).is_some() {
        (
            value_storage(body, block, (destination, target), &kind, span),
            false,
        )
    } else if destination.projection.is_empty() && register_scalar_kind(&kind) {
        (destination, true)
    } else {
        let slot_kind = if register_scalar_kind(&kind) {
            kind
        } else {
            TypeKind::Int
        };
        let slot = Place::new(body.new_local(LocalDecl::new(Type::new(slot_kind, span), span)));
        if let Some(target) = target {
            assign_after_call(body, block, target, destination, slot.clone(), span);
        }
        (slot, true)
    };
    let nothing = body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    append_result_storage(
        body,
        block,
        storage,
        is_out,
        byte_count(layout.payload, span),
    );
    set_call_destination(body, block, Place::new(nothing));
    Ok(())
}

/// The address the runtime writes an element held as its bytes into, taken
/// in `block` ahead of the call: the destination's own bytes when it is a
/// whole local, otherwise a value local of `kind` the destination is assigned
/// from once the call returns.
fn value_storage(
    body: &mut Body,
    block: usize,
    (destination, target): (Place, Option<BasicBlock>),
    kind: &TypeKind,
    span: Span,
) -> Place {
    let bytes = if destination.projection.is_empty() {
        destination
    } else {
        let slot = Place::new(body.new_local(LocalDecl::new(Type::new(kind.clone(), span), span)));
        if let Some(target) = target {
            assign_after_call(body, block, target, destination, slot.clone(), span);
        }
        slot
    };
    let address = body.new_local(LocalDecl::new(Type::new(TypeKind::RawPtr, span), span));
    push_assign(body, block, Place::new(address), Rvalue::Ref(bytes), span);
    Place::new(address)
}

/// The destination, successor and span of the call ending `block`, when it
/// calls a runtime entry point that hands an element back.
fn element_returning_call(body: &Body, block: usize) -> Option<(Place, Option<BasicBlock>, Span)> {
    let terminator = body.basic_blocks[block].terminator.as_ref()?;
    let TerminatorKind::Call {
        func,
        destination,
        target,
        ..
    } = &terminator.kind
    else {
        return None;
    };
    if !crate::runtime_fns::returns_element_value(func.called_runtime_name()?) {
        return None;
    }
    Some((destination.clone(), *target, terminator.span))
}

/// The type of the element a call's destination receives.
///
/// A whole local answers with its declared type; a projected destination with
/// the value held as its bytes or the scalar its projection reaches, or a
/// value word for anything held by reference.
fn destination_kind(body: &Body, destination: &Place) -> Result<TypeKind, String> {
    if destination.projection.is_empty() {
        return Ok(body.local_decls[destination.local.0].ty.kind.clone());
    }
    if let Some(value) = projected_value(body, destination) {
        return Ok(value);
    }
    let reached = Operand::Copy(destination.clone())
        .ty_projected(body)
        .ok_or_else(|| {
            format!(
                "the collection element result {destination} reaches a type this pass cannot size"
            )
        })?;
    Ok(register_scalar(&reached).unwrap_or(TypeKind::Int))
}

/// Whether a value of type `kind` is one scalar held in one register.
fn register_scalar_kind(kind: &TypeKind) -> bool {
    register_scalar(&MirType::from_type_kind(kind)).is_some()
}

/// Route the call ending `block` through a new block that assigns
/// `destination` from `slot` before continuing to `target`.
fn assign_after_call(
    body: &mut Body,
    block: usize,
    target: BasicBlock,
    destination: Place,
    slot: Place,
    span: Span,
) {
    let mut copy =
        BasicBlockData::new(Some(Terminator::new(TerminatorKind::Goto { target }, span)));
    copy.statements.push(Statement {
        kind: StatementKind::Assign(destination, Rvalue::Use(Operand::Copy(slot))),
        span,
    });
    let copy_block = BasicBlock(body.basic_blocks.len());
    body.basic_blocks.push(copy);
    if let Some(Terminator {
        kind: TerminatorKind::Call { target, .. },
        ..
    }) = body.basic_blocks[block].terminator.as_mut()
    {
        *target = Some(copy_block);
    }
}

/// Append the result storage and its byte count as the last two arguments of
/// the call ending `block`, marking the storage `out` when the call writes it
/// back into a register-held place.
fn append_result_storage(
    body: &mut Body,
    block: usize,
    storage: Place,
    is_out: bool,
    payload: Operand,
) {
    let Some(Terminator {
        kind: TerminatorKind::Call {
            args, arg_modes, ..
        },
        ..
    }) = body.basic_blocks[block].terminator.as_mut()
    else {
        return;
    };
    if is_out {
        arg_modes.resize(args.len(), ArgMode::Borrowed);
        arg_modes.extend([ArgMode::Out, ArgMode::Borrowed]);
    }
    args.push(Operand::Copy(storage));
    args.push(payload);
}

/// Make `place` the destination of the call ending `block`.
fn set_call_destination(body: &mut Body, block: usize, place: Place) {
    if let Some(Terminator {
        kind: TerminatorKind::Call { destination, .. },
        ..
    }) = body.basic_blocks[block].terminator.as_mut()
    {
        *destination = place;
    }
}
