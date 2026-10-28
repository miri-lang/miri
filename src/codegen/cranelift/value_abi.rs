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
//!   caller's own bytes — a whole local, or a field of one at its fixed
//!   offset — which the callee only reads. MIR hands the call a copy wherever
//!   the call itself could write those bytes, and a callee that writes such a
//!   parameter copies it on entry.
//! - A result of such a value comes back in registers, one per scalar member
//!   in offset order, when its members fit two integer and two float
//!   registers and none is wider than a register: the callee builds it in a
//!   slot of its own frame and loads the members out of it, and the caller
//!   stores them into its destination's bytes. Any other such result travels
//!   through a hidden leading parameter: the address of the caller's slot,
//!   which the callee writes, returning nothing in registers.
//! - Every other value is passed and returned as one word of its own type.

use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::codegen::cranelift::translator::{FunctionTranslator, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::codegen::cranelift::value_locals::{
    scalar_members, scalar_type, value_layout, ValueLocals,
};
use crate::error::CodegenError;
use crate::mir::body::written_back_out_params;
use crate::mir::instantiation::type_argument;
use crate::mir::type_facts::{ScalarKind, ValueLayout};
use crate::mir::value_arguments::{argument_place, exposure_of, Exposure};
use crate::mir::{ArgMode, Body, Local, Operand, Place};
use cranelift_codegen::ir::{
    types as cl_types, AbiParam, InstBuilder, MemFlagsData, Signature, Value,
};
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
    /// A value held as its bytes, one register per scalar member.
    Lanes(Lanes),
    /// Written into the slot whose address the caller passes ahead of every
    /// parameter.
    ThroughSlot,
}

/// The most integer registers a result held as its bytes comes back in.
const INTEGER_LANES: usize = 2;
/// The most float registers a result held as its bytes comes back in.
const FLOAT_LANES: usize = 2;
/// The widest scalar a register carries, in bytes.
const LANE_BYTES: u32 = 8;

/// The scalar members of a result held as its bytes that comes back in
/// registers, each with its byte offset in the value, in offset order: an
/// optional's tag is the one-byte integer it is stored as.
///
/// Every default convention Miri targets returns two integer and two float
/// registers, so a value whose members fit them crosses a call without
/// touching memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lanes {
    members: [(u32, cl_types::Type); INTEGER_LANES + FLOAT_LANES],
    count: usize,
}

/// The register class a scalar member comes back in.
enum LaneClass {
    Integer,
    Float,
}

impl Lanes {
    /// The lanes a value laid out as `layout` comes back in, or `None` when
    /// its members need more registers than the budget, or one is wider than
    /// a register.
    pub fn of(layout: &ValueLayout) -> Option<Self> {
        let mut lanes = Self {
            members: [(0, cl_types::INVALID); INTEGER_LANES + FLOAT_LANES],
            count: 0,
        };
        let (mut integers, mut floats) = (0, 0);
        for (offset, scalar) in scalar_members(layout) {
            match lane_class(scalar)? {
                LaneClass::Integer => integers += 1,
                LaneClass::Float => floats += 1,
            }
            if integers > INTEGER_LANES || floats > FLOAT_LANES {
                return None;
            }
            *lanes.members.get_mut(lanes.count)? = (offset, scalar_type(scalar));
            lanes.count += 1;
        }
        Some(lanes)
    }

    /// Each member's byte offset and the type it travels as.
    pub fn members(&self) -> &[(u32, cl_types::Type)] {
        &self.members[..self.count]
    }

    /// The types the members travel as, in order.
    fn types(&self) -> impl Iterator<Item = cl_types::Type> + '_ {
        self.members().iter().map(|&(_, ty)| ty)
    }
}

/// The register class `scalar` comes back in, or `None` when no register
/// carries it whole.
fn lane_class(scalar: ScalarKind) -> Option<LaneClass> {
    match scalar {
        ScalarKind::Boolean => Some(LaneClass::Integer),
        ScalarKind::Signed(bytes) | ScalarKind::Unsigned(bytes) => {
            (bytes <= LANE_BYTES).then_some(LaneClass::Integer)
        }
        ScalarKind::Float(bytes) => matches!(bytes, 4 | 8).then_some(LaneClass::Float),
    }
}

/// How a result held as its bytes comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldResult {
    /// In registers, one per scalar member.
    Lanes(Lanes),
    /// Through the slot whose address the caller passes ahead of every
    /// parameter.
    ThroughSlot,
}

impl HeldResult {
    /// How a value laid out as `layout` comes back: in registers when its
    /// members fit them, through the caller's slot otherwise.
    pub fn of(layout: &ValueLayout) -> Self {
        Lanes::of(layout).map_or(Self::ThroughSlot, Self::Lanes)
    }

    /// The same, as a signature's result.
    pub fn returning(self) -> Returning {
        match self {
            Self::Lanes(lanes) => Returning::Lanes(lanes),
            Self::ThroughSlot => Returning::ThroughSlot,
        }
    }
}

/// How a result of type `ty` comes back, `layout` being the layout the code
/// naming it holds a `ty` value in as its bytes, if it does.
///
/// The one classification every signature's result goes through: a body's
/// own, and the one a caller builds from a function type, so the two agree
/// whenever they agree on the layout.
fn result_returning(ty: &Type, layout: Option<ValueLayout>, ptr_type: cl_types::Type) -> Returning {
    if ty.kind == TypeKind::Void {
        return Returning::Nothing;
    }
    match layout {
        Some(layout) => HeldResult::of(&layout).returning(),
        None => Returning::Word(translate_type(ty, ptr_type)),
    }
}

/// Load each lane of the value held as its bytes at `address`.
pub(crate) fn load_lanes(
    builder: &mut FunctionBuilder,
    lanes: &Lanes,
    address: Value,
) -> Vec<Value> {
    lanes
        .members()
        .iter()
        .map(|&(offset, ty)| {
            builder
                .ins()
                .load(ty, MemFlagsData::new(), address, offset as i32)
        })
        .collect()
}

/// Store `values`, one per lane, at their offsets in the bytes at `address`.
pub(crate) fn store_lanes(
    builder: &mut FunctionBuilder,
    lanes: &Lanes,
    values: &[Value],
    address: Value,
) -> Result<(), CodegenError> {
    if values.len() != lanes.members().len() {
        return Err(CodegenError::Internal(format!(
            "a call returned {} values for a result of {} members",
            values.len(),
            lanes.members().len()
        )));
    }
    for (&(offset, _), &value) in lanes.members().iter().zip(values) {
        builder
            .ins()
            .store(MemFlagsData::new(), value, address, offset as i32);
    }
    Ok(())
}

/// Where a call's result goes: how it comes back, and the address of the
/// destination's bytes when it is a value held as its bytes — the return
/// slot when it comes back through one, where its lanes are stored when it
/// comes back in registers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CallResult {
    pub(crate) returning: Returning,
    pub(crate) address: Option<Value>,
}

impl CallResult {
    /// A call with no result.
    pub(crate) fn nothing() -> Self {
        Self {
            returning: Returning::Nothing,
            address: None,
        }
    }

    /// The return slot passed ahead of the call's arguments, when the result
    /// comes back through one.
    pub(crate) fn return_slot(&self) -> Option<Value> {
        match self.returning {
            Returning::ThroughSlot => self.address,
            Returning::Nothing | Returning::Word(_) | Returning::Lanes(_) => None,
        }
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Where the result of a call into `destination` goes.
    ///
    /// A destination holding a value as its bytes receives the value in
    /// registers or as the callee's return slot, as its layout decides. A
    /// runtime function returns no such value, and a call's result is never
    /// written into a scalar inside one: either is refused.
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
            let layout = spot.value().filter(|_| !is_runtime).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "a call's result is written into `{destination}` inside a value held as \
                     its bytes, which only a Miri function returning that value fills"
                ))
            })?;
            return Ok(CallResult {
                returning: HeldResult::of(layout).returning(),
                address: Some(address),
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
            address: None,
        })
    }

    /// The address a value argument held as its bytes at `address` is passed
    /// as: the caller's own bytes, which an `out` argument's callee writes in
    /// place and any other callee only reads.
    ///
    /// MIR guarantees a non-`out` value argument is a value local, or a field
    /// of one, that the call writes neither as its destination nor as an
    /// `out` argument ([`Self::refuse_exposed_value_argument`] holds the call
    /// to it), and a callee that writes a parameter copies it first
    /// (`written_value_params`). Closures capture by value, so no other path
    /// reaches a local's own bytes during the call; a value parameter's bytes,
    /// or a field of them, are ones its caller already guarantees no one
    /// writes during this call.
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
    /// Refuse `arg`, passed under `mode` to a call into `destination` that
    /// writes the root locals `written`, in the code whose body is `body`,
    /// when it is a non-`out` value argument whose bytes the call could change
    /// under its callee, as [`exposure_of`] decides. The value-argument pass
    /// copies every such argument first, so reaching one here is a compiler
    /// bug.
    pub(crate) fn refuse_exposed_value_argument(
        arg: &Operand,
        mode: ArgMode,
        destination: &Place,
        written: &[Local],
        body: Option<&Body>,
    ) -> Result<(), CodegenError> {
        if mode == ArgMode::Out {
            return Ok(());
        }
        let Some(place) = argument_place(arg) else {
            return Ok(());
        };
        let body = body.ok_or_else(|| {
            CodegenError::Internal(format!(
                "the value argument `{place}` is passed by code with no body of its own"
            ))
        })?;
        let problem = match exposure_of(body, place, destination, written) {
            None => return Ok(()),
            Some(Exposure::Projected) => "reaches the call from inside its holder",
            Some(Exposure::Destination | Exposure::Out) => "is aliased with what the call writes",
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
    match result {
        Returning::Nothing | Returning::ThroughSlot => {}
        Returning::Word(word) => signature.returns.push(AbiParam::new(word)),
        Returning::Lanes(lanes) => signature.returns.extend(lanes.types().map(AbiParam::new)),
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
/// travels through one ([`CallResult::return_slot`]), then `args`.
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
    result_returning(&decl.ty, value_layout(body, &decl.ty.kind), ptr_type)
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
    result_returning(ty, values.type_layout(&ty.kind), ptr_type)
}
