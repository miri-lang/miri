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
//! The positions come from [`crate::runtime_fns::element_positions`]; the
//! width, and whether the operand is already an address, from
//! [`crate::ast::types::element_layout`]. The pass runs after reference
//! counting, which has already accounted for the element as the value it is.

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::{element_layout, ElementLayout, Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::{
    Body, Constant, LocalDecl, MirType, Operand, Place, Rvalue, Statement, StatementKind,
    TerminatorKind,
};

/// Rewrite every element argument in `body` into its address and byte count.
///
/// An element that cannot be sized — an inline vector read through a
/// projection, whose component width the projected type does not record — is
/// reported rather than guessed at.
pub fn pass_elements_by_address(body: &mut Body) -> Result<(), String> {
    for block in 0..body.basic_blocks.len() {
        let Some((positions, args, span)) = element_call(body, block) else {
            continue;
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
    }
    Ok(())
}

/// The element positions, arguments and span of the call ending `block`, when
/// it calls a runtime entry point that takes an element.
fn element_call(body: &Body, block: usize) -> Option<(&'static [usize], Vec<Operand>, Span)> {
    let terminator = body.basic_blocks[block].terminator.as_ref()?;
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return None;
    };
    let positions = crate::runtime_fns::element_positions(func.called_symbol()?);
    if positions.is_empty() {
        return None;
    }
    Some((positions, args.clone(), terminator.span))
}

/// Install `expanded` as the arguments of the call ending `block`, widening its
/// per-argument metadata to match: each element position gained one argument
/// after it, which is neither an `out` parameter nor a device handle.
fn replace_call_args(body: &mut Body, block: usize, expanded: Vec<Operand>, positions: &[usize]) {
    let Some(terminator) = body.basic_blocks[block].terminator.as_mut() else {
        return;
    };
    let TerminatorKind::Call {
        args,
        out_args,
        arg_handles,
        ..
    } = &mut terminator.kind
    else {
        return;
    };
    for &position in positions.iter().rev() {
        if position < out_args.len() {
            out_args.insert(position + 1, false);
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
/// TODO: a collection method compiled once for every element type — `Set`'s
/// own `contains` and `remove`, which forward their parameter to the runtime —
/// declares its element at the bare type parameter, so its layout here is a
/// value word. An inline vector reaching such a method arrives as the address
/// of its components, and handing over the address of that address matches no
/// stored element. Giving the method body its instantiation's element types is
/// what closes it.
fn element_address(
    body: &mut Body,
    block: usize,
    arg: Operand,
    span: Span,
) -> Result<(Operand, Operand), String> {
    let layout = argument_layout(body, &arg)?;
    let payload = byte_count(layout.payload, span);
    if layout.is_address {
        return Ok((arg, payload));
    }
    let place = match arg {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Constant(constant) => {
            let temp = body.new_local(LocalDecl::new(constant.ty.clone(), span));
            let value = Rvalue::Use(Operand::Constant(constant));
            push_assign(body, block, Place::new(temp), value, span);
            Place::new(temp)
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
            Ok(element_layout(&body.local_decls[place.local.0].ty.kind))
        }
        Operand::Constant(constant) => Ok(element_layout(&constant.ty.kind)),
        Operand::Copy(place) | Operand::Move(place) => {
            let reached = arg.ty_projected(body).ok_or_else(|| {
                format!("the collection element {place} reaches a type this pass cannot size")
            })?;
            projected_layout(&reached).ok_or_else(|| {
                format!(
                    "the collection element {place} is an inline vector read through a \
                     projection, whose component width is not known here"
                )
            })
        }
    }
}

/// The layout of an element whose type is known only as the MIR type a
/// projection reaches, or `None` for an inline vector, whose components that
/// type does not record.
fn projected_layout(reached: &MirType) -> Option<ElementLayout> {
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
        MirType::Custom(name) if crate::ast::types::vec_dim(name).is_some() => return None,
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
        | MirType::Unknown => TypeKind::RawPtr,
    };
    Some(element_layout(&scalar))
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
