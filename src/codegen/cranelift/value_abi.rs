// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a Miri function's parameters and result cross a call.
//!
//! Every signature a Miri function is declared, defined or called under is
//! built by [`miri_signature`], so the callee's own signature, a direct call, a
//! call through a vtable slot and a call through a closure's function pointer
//! cannot disagree about where a value goes. A runtime function keeps the C
//! signature its declaration spells, and is not built here.
//!
//! - A bitwise value held as its bytes is passed as the address of the
//!   caller's own bytes, which the callee only reads. MIR hands the call a
//!   copy wherever the call itself could write those bytes, and a callee that
//!   writes such a parameter copies it on entry.
//! - A result of such a value travels through a hidden leading parameter: the
//!   address of the caller's slot, which the callee writes. The callee returns
//!   nothing in registers.
//! - Every other value is passed and returned as one word of its own type.

use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::codegen::cranelift::translator::{FunctionTranslator, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::codegen::cranelift::value_locals::{value_layout, ValueLocals};
use crate::error::CodegenError;
use crate::mir::body::written_back_out_params;
use crate::mir::instantiation::type_argument;
use crate::mir::terminator::arg_mode;
use crate::mir::value_arguments::{argument_place, call_written_roots};
use crate::mir::{ArgMode, Body, Local, Operand, Place};
use cranelift_codegen::ir::{types as cl_types, AbiParam, Signature, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;

/// How one parameter of a Miri function is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passing {
    /// One word of this type.
    Word(cl_types::Type),
    /// The address of the bytes of a value.
    ByAddress,
}

/// How the result of a Miri function comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returning {
    /// There is no result.
    Nothing,
    /// One word of this type.
    Word(cl_types::Type),
    /// Written into the slot whose address the caller passes ahead of every
    /// parameter.
    ThroughSlot,
}

/// Where a call's result goes: how it comes back, and the address of the
/// return slot when it comes back through one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallResult {
    pub(crate) returning: Returning,
    pub(crate) slot: Option<Value>,
}

impl CallResult {
    /// A call with no result.
    pub(crate) fn nothing() -> Self {
        Self {
            returning: Returning::Nothing,
            slot: None,
        }
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Where the result of a call into `destination` goes.
    ///
    /// A destination holding a value as its bytes is the callee's return
    /// slot. A runtime function returns no such value, and a call's result is
    /// never written into a scalar inside one: either is refused.
    pub(crate) fn call_result(
        builder: &mut FunctionBuilder,
        destination: &Place,
        is_runtime: bool,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<CallResult, CodegenError> {
        if let Some((address, spot)) =
            Self::value_place_address(builder, destination, locals, type_ctx)?
        {
            if is_runtime || spot.value().is_none() {
                return Err(CodegenError::Internal(format!(
                    "a call's result is written into `{destination}` inside a value held as \
                     its bytes, which only a Miri function returning that value fills"
                )));
            }
            return Ok(CallResult {
                returning: Returning::ThroughSlot,
                slot: Some(address),
            });
        }
        if !destination.projection.is_empty() {
            let reached = Self::resolve_projected_type_kind(destination, type_ctx)?;
            if type_ctx.value_layout(&reached).is_some() {
                return Err(CodegenError::Internal(format!(
                    "a call's result is written straight into `{destination}`, a value held \
                     inline in its holder, which only a value local receives"
                )));
            }
        }
        let dest_ty = type_ctx.local_types[destination.local.0];
        let returning = if dest_ty.kind == TypeKind::Void {
            Returning::Nothing
        } else {
            Returning::Word(translate_type(dest_ty, type_ctx.ptr_type))
        };
        Ok(CallResult {
            returning,
            slot: None,
        })
    }

    /// The address a value argument held as its bytes at `address` is passed
    /// as: the caller's own bytes, which an `out` argument's callee writes in
    /// place and any other callee only reads.
    ///
    /// MIR guarantees a non-`out` value argument is a whole local that the
    /// call writes neither as its destination nor as an `out` argument
    /// ([`Self::refuse_exposed_value_argument`] holds the call to it), and a
    /// callee that writes a parameter copies it first (`written_value_params`).
    /// Closures capture by value, so no other path reaches these bytes during
    /// the call.
    ///
    /// A runtime function takes no such value, and a value is never handed
    /// over owned, since only a counted local is: either is refused.
    pub(crate) fn value_argument(
        address: Value,
        mode: ArgMode,
        is_runtime: bool,
    ) -> Result<Value, CodegenError> {
        if is_runtime {
            return Err(CodegenError::Internal(
                "a value held as its bytes is passed to a runtime function".to_string(),
            ));
        }
        match mode {
            ArgMode::Out | ArgMode::Borrowed => Ok(address),
            ArgMode::Owned => Err(CodegenError::Internal(
                "a value held as its bytes is passed owned, which only a counted local is"
                    .to_string(),
            )),
        }
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Refuse argument `position` of a call taking `args` under `arg_modes`
    /// into `destination` when it is a non-`out` value argument whose bytes
    /// the call could change under its callee: one read from inside its
    /// holder, or from a local the call writes as its destination or through
    /// an `out` argument. The value-argument pass copies every such argument
    /// first, so reaching one here is a compiler bug.
    pub(crate) fn refuse_exposed_value_argument(
        position: usize,
        args: &[Operand],
        arg_modes: &[ArgMode],
        destination: &Place,
    ) -> Result<(), CodegenError> {
        if arg_mode(arg_modes, position) == ArgMode::Out {
            return Ok(());
        }
        let Some(place) = args.get(position).and_then(argument_place) else {
            return Ok(());
        };
        let problem = if !place.projection.is_empty() {
            "reaches the call from inside its holder"
        } else if call_written_roots(args, arg_modes, destination).any(|root| root == place.local) {
            "is aliased with what the call writes"
        } else {
            return Ok(());
        };
        Err(CodegenError::Internal(format!(
            "the value argument `{place}` {problem}; the value-argument pass should have \
             copied it"
        )))
    }
}

/// The signature of a Miri function taking `params` and giving `result`.
pub fn miri_signature(
    call_conv: CallConv,
    ptr_type: cl_types::Type,
    params: &[Passing],
    result: Returning,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    if result == Returning::ThroughSlot {
        signature.params.push(AbiParam::new(ptr_type));
    }
    signature.params.extend(
        params
            .iter()
            .map(|passing| AbiParam::new(passing.word(ptr_type))),
    );
    if let Returning::Word(word) = result {
        signature.returns.push(AbiParam::new(word));
    }
    signature
}

impl Passing {
    /// The Cranelift type the parameter travels as.
    fn word(self, ptr_type: cl_types::Type) -> cl_types::Type {
        match self {
            Passing::Word(word) => word,
            Passing::ByAddress => ptr_type,
        }
    }
}

/// The arguments of a call: the return slot's address first when the result
/// travels through one, then `args`.
pub fn call_arguments(return_slot: Option<Value>, args: &[Value]) -> Vec<Value> {
    return_slot
        .into_iter()
        .chain(args.iter().copied())
        .collect()
}

/// The signature `body` is defined under, and declared under for every
/// caller.
///
/// An `out` parameter is the address of the caller's slot whatever it holds.
pub fn body_signature(body: &Body, call_conv: CallConv, ptr_type: cl_types::Type) -> Signature {
    let written_back = written_back_out_params(body);
    let params: Vec<Passing> = (1..=body.arg_count)
        .filter_map(|index| body.local_decls.get(index).map(|decl| (Local(index), decl)))
        .map(|(local, decl)| {
            if value_layout(body, &decl.ty.kind).is_some() {
                Passing::ByAddress
            } else if written_back.contains(&local) {
                Passing::Word(ptr_type)
            } else {
                Passing::Word(translate_type(&decl.ty, ptr_type))
            }
        })
        .collect();
    miri_signature(call_conv, ptr_type, &params, body_result(body, ptr_type))
}

/// How the result of `body`, its local `_0`, comes back.
fn body_result(body: &Body, ptr_type: cl_types::Type) -> Returning {
    let Some(decl) = body.local_decls.first() else {
        return Returning::Nothing;
    };
    if decl.ty.kind == TypeKind::Void {
        Returning::Nothing
    } else if value_layout(body, &decl.ty.kind).is_some() {
        Returning::ThroughSlot
    } else {
        Returning::Word(translate_type(&decl.ty, ptr_type))
    }
}

/// The signature of a function of type `function`, with `leading` passed
/// ahead of its declared parameters, as the code being translated holds the
/// types it names.
pub fn function_type_signature(
    function: &FunctionTypeData,
    leading: &[Passing],
    values: &ValueLocals,
    call_conv: CallConv,
    ptr_type: cl_types::Type,
) -> Signature {
    let declared = function
        .params
        .iter()
        .filter_map(|param| written_type(&param.typ))
        .map(|param_type| passing_of(&param_type, values, ptr_type));
    let params: Vec<Passing> = leading.iter().copied().chain(declared).collect();
    let result = match function.return_type.as_deref().and_then(written_type) {
        Some(ret_type) => returning_of(&ret_type, values, ptr_type),
        None => Returning::Nothing,
    };
    miri_signature(call_conv, ptr_type, &params, result)
}

/// The type a function type's parameter or result is written as: `T?` is
/// the type with its nullable flag set, read as the optional it spells.
fn written_type(expr: &crate::ast::expression::Expression) -> Option<Type> {
    type_argument(expr)
}

/// How a parameter of type `ty` is passed.
fn passing_of(ty: &Type, values: &ValueLocals, ptr_type: cl_types::Type) -> Passing {
    match values.type_layout(&ty.kind) {
        Some(_) => Passing::ByAddress,
        None => Passing::Word(translate_type(ty, ptr_type)),
    }
}

/// How a result of type `ty` comes back.
fn returning_of(ty: &Type, values: &ValueLocals, ptr_type: cl_types::Type) -> Returning {
    if ty.kind == TypeKind::Void {
        Returning::Nothing
    } else if values.type_layout(&ty.kind).is_some() {
        Returning::ThroughSlot
    } else {
        Returning::Word(translate_type(ty, ptr_type))
    }
}
