// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The MIR a readback is spelled in.
//!
//! A readback is a call, and a call ends its block, so every helper here takes
//! the block the readback is appended to and returns the block that continues
//! after it.

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::type_arguments::{TypeArgKind, ValueArg};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::body::DeviceHandleId;
use crate::mir::compiler_intrinsics::{self as intrinsics, Intrinsic};
use crate::mir::{
    AggregateKind, BasicBlock, BasicBlockData, Body, Constant, Local, LocalDecl, Operand, Place,
    PlaceElem, Rvalue, Statement, StatementKind, Terminator,
};

/// Append to block `from` the readback of `local`'s device buffer into its host
/// value, returning the block that continues after it. A local with no device
/// handle needs nothing and gets nothing: `from` itself is returned.
///
/// An array the body owns is first given a host array of its own when anything
/// else shares it. Every earlier host copy of the binding — `let h = g`, a tuple
/// holding it, a closure's capture — shares that array by reference count, and
/// a readback written in place would rewrite each of them to the later results.
/// The detach is copy-on-write: an array nothing else holds is read back in
/// place, so a readback costs no allocation and no extra host copy unless a
/// copy is actually needed. A parameter's array belongs to the caller and is
/// read back in place.
///
/// A scalar reads back through a one-element array, since the runtime entry
/// copies a device buffer into a host array.
pub(crate) fn append_readback(
    body: &mut Body,
    from: BasicBlock,
    local: Local,
    span: Span,
) -> BasicBlock {
    let Some(handle) = body.local_decls[local.0].device_handle else {
        return from;
    };
    if !is_array(&body.local_decls[local.0].ty) {
        return append_scalar_readback(body, from, local, handle, span);
    }
    let from = if local.0 > body.arg_count {
        detach_host_array(body, from, local, span)
    } else {
        from
    };
    let args = vec![
        handle_operand(handle, span),
        Operand::Copy(Place::new(local)),
    ];
    append_void_call(body, from, &intrinsics::READBACK, args, span)
}

fn is_array(ty: &Type) -> bool {
    matches!(ty.kind, TypeKind::Array(_, _))
        || matches!(&ty.kind, TypeKind::Custom(name, _)
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array))
}

/// Make `local`'s host array one no other value shares, copying it only when
/// something does.
///
/// The copy-on-write entry takes the local's reference over (`move`, passed
/// owned as its declaration states) and hands back one — the same array or a
/// fresh copy, having released the original — so the result is stored back
/// with `Assign`, which releases nothing, from a `Move`, which retains nothing.
fn detach_host_array(body: &mut Body, from: BasicBlock, local: Local, span: Span) -> BasicBlock {
    let ty = body.local_decls[local.0].ty.clone();
    let unshared = body.new_local(LocalDecl::new(ty, span));
    let args = vec![Operand::Move(Place::new(local))];
    let next = append_call(
        body,
        from,
        &intrinsics::ARRAY_COW,
        args,
        Place::new(unshared),
        span,
    );
    body.basic_blocks[next.0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Move(Place::new(unshared))),
        ),
        span,
    });
    next
}

/// Read a gpu scalar back through a one-element `Array<T, 1>` wrapper, then
/// copy element 0 into the scalar.
///
/// The wrapper is seeded with the scalar's own host value. A scalar no launch
/// has touched has no device buffer, and the runtime then leaves the wrapper as
/// it found it; the unconditional copy back must hand the scalar its own value,
/// not a placeholder the readback never overwrote.
fn append_scalar_readback(
    body: &mut Body,
    from: BasicBlock,
    local: Local,
    handle: DeviceHandleId,
    span: Span,
) -> BasicBlock {
    let scalar_ty = body.local_decls[local.0].ty.clone();
    let wrapper = body.new_local(LocalDecl::new(one_element_array(scalar_ty, span), span));
    body.basic_blocks[from.0].statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(wrapper),
            Rvalue::Aggregate(AggregateKind::Array, vec![Operand::Copy(Place::new(local))]),
        ),
        span,
    });
    let args = vec![
        handle_operand(handle, span),
        Operand::Copy(Place::new(wrapper)),
    ];
    let next = append_void_call(body, from, &intrinsics::READBACK, args, span);

    let index = body.new_local(LocalDecl::new(Type::new(TypeKind::Int, span), span));
    let mut element = Place::new(wrapper);
    element.projection.push(PlaceElem::index(index));
    body.basic_blocks[next.0].statements.extend([
        Statement {
            kind: StatementKind::Assign(Place::new(index), Rvalue::Use(int_constant(0, span))),
            span,
        },
        Statement {
            kind: StatementKind::Assign(Place::new(local), Rvalue::Use(Operand::Copy(element))),
            span,
        },
        Statement {
            kind: StatementKind::storage_dead(Place::new(wrapper)),
            span,
        },
    ]);
    next
}

/// `Array<T, 1>` for element type `element`.
fn one_element_array(element: Type, span: Span) -> Type {
    let type_arg = |node| crate::ast::IdNode::new(0, node, span);
    Type::new(
        TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![
                type_arg(TypeArgKind::Type(Box::new(element), false)),
                type_arg(TypeArgKind::Value(ValueArg::Int(1))),
            ]),
        ),
        span,
    )
}

/// End block `from` with a call to runtime entry `intrinsic` whose result is
/// discarded, returning the block that continues after it.
///
/// Terminator operands are not retained by RC insertion, so a managed argument is
/// borrowed and survives the call.
fn append_void_call(
    body: &mut Body,
    from: BasicBlock,
    intrinsic: &Intrinsic,
    args: Vec<Operand>,
    span: Span,
) -> BasicBlock {
    let discarded = body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    append_call(body, from, intrinsic, args, Place::new(discarded), span)
}

/// End block `from` with a call to runtime entry `intrinsic` storing into
/// `destination`, returning the block that continues after it.
fn append_call(
    body: &mut Body,
    from: BasicBlock,
    intrinsic: &Intrinsic,
    args: Vec<Operand>,
    destination: Place,
    span: Span,
) -> BasicBlock {
    let func = Operand::runtime(intrinsic.callee(), span);
    let next = new_block(body);
    body.basic_blocks[from.0].terminator = Some(Terminator::runtime_call(
        func,
        args,
        destination,
        Some(next),
        span,
    ));
    next
}

/// Append an empty block with no terminator yet.
pub(super) fn new_block(body: &mut Body) -> BasicBlock {
    body.basic_blocks.push(BasicBlockData::new(None));
    BasicBlock(body.basic_blocks.len() - 1)
}

/// `flag = value`.
pub(super) fn flag_assignment(flag: Local, value: bool, span: Span) -> Statement {
    Statement {
        kind: StatementKind::Assign(
            Place::new(flag),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::Boolean, span),
                literal: Literal::Boolean(value),
            }))),
        ),
        span,
    }
}

/// The operand naming device handle `handle`, as the runtime entries take it.
fn handle_operand(handle: DeviceHandleId, span: Span) -> Operand {
    int_constant(handle.0 as i64, span)
}

/// An `int`-typed integer constant operand.
fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}
