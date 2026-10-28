// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Locals that hold a bitwise value as its bytes, in a stack slot of the
//! frame.
//!
//! A *value local* is one whose declared type the value-class authority calls
//! bitwise and whose body's classifier does not count. Its Cranelift
//! `Variable` holds the address of its bytes: a slot of this frame sized and
//! aligned by the value's [`ValueLayout`], the pointer to the copy a caller
//! made for a parameter, or, for a `_0` that comes back through one, the
//! caller's return slot.
//!
//! - Reading a value place yields the address of its bytes; reading a scalar
//!   inside one loads it from its offset in the layout.
//! - Assigning a value copies its bytes into the destination; an aggregate of
//!   a value type is built straight into the destination's bytes.
//! - No count is kept for a value, and its address never reaches a word slot
//!   that outlives the frame: either is refused as an internal error.
//!
//! A value held inline in a class, a struct, a tuple, a collection, a payload
//! or a closure's environment is reached the same way, through the address its
//! holder's layout places it at.

use crate::ast::types::{Type, TypeKind};
use crate::codegen::cranelift::held_enums::{
    collect_column_members, member_read_as, projected_column_value,
};
use crate::codegen::cranelift::inline_optionals::is_absent;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::value_abi::HeldResult;
use crate::error::CodegenError;
use crate::mir::analysis::copies_read_in_place::copies_read_in_place;
use crate::mir::type_facts::{FieldRepr, ScalarKind, ValueField, ValueLayout};
use crate::mir::{AggregateKind, Body, Local, Operand, Place, PlaceElem, Rvalue};
use cranelift_codegen::ir::immediates::Ieee16;
use cranelift_codegen::ir::{
    types as cl_types, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::{HashMap, HashSet};

impl<'a> FunctionTranslator<'a> {
    /// Give every value local that is not a parameter a stack slot of this
    /// frame, and set its `Variable` to the slot's address. The result `_0`
    /// gets one too unless it comes back through the caller's slot.
    pub(crate) fn define_value_slots(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        values: &ValueLocals,
        ptr_type: cl_types::Type,
    ) {
        let result = (values.held_result() != Some(HeldResult::ThroughSlot)).then_some(Local(0));
        let owned = result
            .into_iter()
            .chain((body.arg_count + 1..body.local_decls.len()).map(Local));
        for local in owned {
            let (Some(layout), Some(&var)) = (values.layout(local), locals.get(&local)) else {
                continue;
            };
            let address = value_slot(builder, layout, ptr_type);
            builder.def_var(var, address);
        }
    }

    /// The address of what `place` reaches inside a value local's bytes, and
    /// what it reaches; `None` when `place` is not rooted in a value local.
    pub(crate) fn value_place_address<'t>(
        builder: &mut FunctionBuilder,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &'t TypeCtx,
    ) -> Result<Option<(Value, ValueSpot<'t>)>, CodegenError> {
        let Some(spot) = type_ctx.values.spot(place)? else {
            return Ok(None);
        };
        if place
            .projection
            .iter()
            .any(|elem| matches!(elem, PlaceElem::Index(_)))
        {
            return Err(CodegenError::Internal(format!(
                "`{place}` picks an element of a value by a value, which has no fixed address"
            )));
        }
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let base = builder.use_var(*var);
        let address = match spot.offset() {
            0 => base,
            offset => builder.ins().iadd_imm_s(base, i64::from(offset)),
        };
        Ok(Some((address, spot)))
    }

    /// Read what `spot` reaches at `address`: a scalar is loaded, a whole
    /// value is its address. An enum payload column whose variants store
    /// different members is read as the member `expected`, the type it is
    /// read into, names.
    pub(crate) fn read_value_spot(
        builder: &mut FunctionBuilder,
        address: Value,
        spot: ValueSpot,
        expected: Option<&Type>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let scalar = match spot {
            ValueSpot::Scalar { scalar, .. } => scalar,
            ValueSpot::Value { .. } => return Ok(address),
            ValueSpot::Column { members, .. } => {
                match member_read_as(members, expected, type_ctx)? {
                    FieldRepr::Scalar(scalar) => *scalar,
                    FieldRepr::Value(_) => return Ok(address),
                    FieldRepr::OptionTag | FieldRepr::Overlay(_) => {
                        return Err(CodegenError::Internal(
                            "an enum payload column holds a tag or a column".to_string(),
                        ))
                    }
                }
            }
        };
        Ok(builder
            .ins()
            .load(scalar_type(scalar), MemFlagsData::new(), address, 0))
    }

    /// The address of the bytes of the whole value `operand` reads — in a
    /// value local, or held inline in a field, an element or a payload —
    /// anything else read where a value's bytes are expected is refused.
    ///
    /// `expected` is the type the bytes are read as, which says what an enum
    /// payload holds.
    pub(crate) fn value_operand_address(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected: Option<&Type>,
    ) -> Result<Value, CodegenError> {
        if type_ctx.values.operand_value(operand)?.is_none() {
            let reached = Self::operand_projected_kind(operand, type_ctx)?;
            let is_held = matches!(operand, Operand::Copy(_) | Operand::Move(_))
                && Self::reaches_held_value(&reached, expected, type_ctx);
            if !is_held {
                return Err(CodegenError::Internal(format!(
                    "`{operand}` is read where a value's bytes are expected, but holds none"
                )));
            }
        }
        Self::translate_operand(builder, ctx, operand, locals, type_ctx, expected)
    }

    /// The address of element `index` of the tuple held as its bytes at
    /// `base`, laid out as `layout`, and what that element holds.
    ///
    /// Only a tuple whose elements are all laid out alike is indexed by a
    /// value: its elements sit one stride apart. An index past the last
    /// element stops the program, as an out-of-bounds read of a collection
    /// does.
    pub(crate) fn value_element_address<'l>(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base: Value,
        index: Value,
        layout: &'l ValueLayout,
    ) -> Result<(Value, &'l FieldRepr), CodegenError> {
        let (stride, element) = uniform_element(layout)?;
        let index_type = builder.func.dfg.value_type(index);
        let count = builder.ins().iconst(index_type, layout.fields.len() as i64);
        Self::trap_out_of_bounds(builder, ctx, index, count)?;
        let elem_offset = builder.ins().imul_imm_s(index, i64::from(stride));
        Ok((builder.ins().iadd(base, elem_offset), element))
    }

    /// Stop the program with an out-of-bounds report unless `index` is below
    /// `count`.
    fn trap_out_of_bounds(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        index: Value,
        count: Value,
    ) -> Result<(), CodegenError> {
        let out_of_bounds = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            index,
            count,
        );
        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(out_of_bounds, panic_block, &[], cont_block, &[]);
        builder.switch_to_block(panic_block);
        Self::call_rt_array_panic_oob(builder, ctx, index, count)?;
        builder
            .ins()
            .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
        builder.switch_to_block(cont_block);
        builder.seal_block(panic_block);
        builder.seal_block(cont_block);
        Ok(())
    }

    /// Read the place rooted in a value local whose projection indexes a
    /// tuple held as its bytes by a value: a scalar is loaded, a whole value
    /// is its address.
    pub(crate) fn read_indexed_value_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let root = type_ctx.values.layout(place.local).ok_or_else(|| {
            CodegenError::Internal(format!("`{place}` is not rooted in a value local"))
        })?;
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let mut address = builder.use_var(*var);
        let mut reached = ReachedRepr::Value(root);
        for elem in &place.projection {
            let ReachedRepr::Value(layout) = reached else {
                return Err(projection_error(elem, "a scalar"));
            };
            let (next, repr) = match elem {
                PlaceElem::Field(index) => {
                    let field = member(layout, *index)?;
                    let next = builder.ins().iadd_imm_s(address, i64::from(field.offset));
                    (next, &field.repr)
                }
                PlaceElem::Index(index) => {
                    let index_var = locals.get(index).ok_or_else(|| {
                        CodegenError::Internal(format!("Unknown index local: {index:?}"))
                    })?;
                    let index_val = builder.use_var(*index_var);
                    Self::value_element_address(builder, ctx, address, index_val, layout)?
                }
                PlaceElem::Deref => {
                    return Err(projection_error(elem, "a value held as its bytes"))
                }
            };
            address = next;
            reached = ReachedRepr::of(repr, elem)?;
        }
        Ok(match reached {
            ReachedRepr::Scalar(scalar) => {
                builder
                    .ins()
                    .load(scalar_type(scalar), MemFlagsData::new(), address, 0)
            }
            ReachedRepr::Value(_) => address,
        })
    }

    /// Refuse a count adjustment of a place rooted in a value local: a value
    /// held as its bytes has no count.
    pub(crate) fn refuse_count_of_value(
        place: &Place,
        operation: &str,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if type_ctx.values.spot(place)?.is_none() {
            return Ok(());
        }
        Err(CodegenError::Internal(format!(
            "{operation} of `{place}`, a value held as its bytes, which has no count"
        )))
    }

    /// Refuse an assignment into a word that reads a whole value held as its
    /// bytes: what it would store is the address of a stack slot, which the
    /// word outlives.
    ///
    /// An aggregate that holds its members in an allocation of its own — a
    /// class, a struct or tuple held as a reference, an enum, a box, a
    /// closure's environment, an array — copies such a value's bytes in, and
    /// is no escape.
    pub(crate) fn refuse_value_escape(
        rvalue: &Rvalue,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let operands: &[Operand] = match rvalue {
            Rvalue::Use(operand) => std::slice::from_ref(operand),
            Rvalue::Cast(operand, _) => std::slice::from_ref(operand.as_ref()),
            Rvalue::Aggregate(
                AggregateKind::Struct(_)
                | AggregateKind::Tuple(_)
                | AggregateKind::Class(_)
                | AggregateKind::Enum(_, _)
                | AggregateKind::Option(_)
                | AggregateKind::Closure(_, _)
                | AggregateKind::Array,
                _,
            ) => return Ok(()),
            Rvalue::Aggregate(
                AggregateKind::FormattedString
                | AggregateKind::List
                | AggregateKind::Map
                | AggregateKind::Set,
                operands,
            ) => operands,
            Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => return Ok(()),
        };
        for operand in operands {
            if type_ctx.values.operand_value(operand)?.is_some() {
                return Err(CodegenError::Internal(format!(
                    "`{operand}`, a value held as its bytes, is stored into a word"
                )));
            }
        }
        Ok(())
    }

    /// Refuse an aggregate that hands a member slot held as its bytes anything
    /// but bytes of that slot's layout: a word, the absent constant into a
    /// counted holder, or a value held at another layout. A value built in
    /// place clears a member the absent constant fills, and is no such store.
    ///
    /// In a body compiled for one instantiation every member's type can be
    /// read; one that cannot is refused rather than stored by guess. A shared
    /// generic body, which leaves parameters open, passes such a member on.
    ///
    /// A closure has no member slots of its own: each capture is stored at
    /// the capture's own type, the type the lambda reads it at, so no capture
    /// can miss its slot's layout.
    pub(crate) fn refuse_word_into_held_slot(
        place: &Place,
        rvalue: &Rvalue,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let (Rvalue::Aggregate(kind, operands), Some(body)) = (rvalue, type_ctx.values.body())
        else {
            return Ok(());
        };
        if matches!(kind, AggregateKind::Closure(_, _)) {
            return Ok(());
        }
        let slots = crate::mir::place_type::PlaceTypes::of(body);
        let builds_in_place = slots
            .declared(place)
            .is_some_and(|ty| type_ctx.value_layout(&ty.kind).is_some());
        let is_monomorphized = body.open_params.is_empty();
        for (index, operand) in operands.iter().enumerate() {
            let Some(slot) = slots.aggregate_slot(kind, place, index) else {
                if is_monomorphized || (is_absent(operand) && !builds_in_place) {
                    return Err(CodegenError::Internal(format!(
                        "`{operand}` is stored into member {index} of `{place}`, whose type \
                         cannot be read"
                    )));
                }
                continue;
            };
            let Some(layout) = type_ctx.value_layout(&slot.kind) else {
                continue;
            };
            let fits = if is_absent(operand) {
                builds_in_place
            } else {
                Self::operand_held_layout(operand, type_ctx)?.as_ref() == Some(&layout)
            };
            if !fits {
                return Err(CodegenError::Internal(format!(
                    "`{operand}` is stored into member {index} of `{place}`, a slot of `{}` \
                     held as its bytes, without bytes of its layout",
                    slot.kind
                )));
            }
        }
        Ok(())
    }

    /// Refuse a virtual call handing a parameter held as its bytes a word —
    /// the absent constant, or a value that is no value held as bytes — at
    /// the parameter type the call site recorded for it.
    pub(crate) fn refuse_word_into_held_parameters(
        args: &[Operand],
        param_types: &[Option<Type>],
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        for (position, (operand, param)) in args.iter().skip(1).zip(param_types).enumerate() {
            let Some(param) = param else {
                continue;
            };
            if type_ctx.value_layout(&param.kind).is_none() {
                continue;
            }
            if is_absent(operand) || Self::operand_held_layout(operand, type_ctx)?.is_none() {
                return Err(CodegenError::Internal(format!(
                    "`{operand}`, a word, is passed as argument {} of a virtual call, a \
                     parameter of `{}` held as its bytes",
                    position + 1,
                    param.kind
                )));
            }
        }
        Ok(())
    }

    /// Translate an assignment into a place rooted in a value local, whose
    /// bytes start at `address`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn assign_value_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        address: Value,
        spot: ValueSpot,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected: Option<&Type>,
    ) -> Result<(), CodegenError> {
        match spot {
            ValueSpot::Scalar { scalar, .. } => {
                let value = Self::translate_rvalue(builder, ctx, rvalue, locals, type_ctx, None)?;
                let is_unsigned = matches!(scalar, ScalarKind::Unsigned(_));
                Self::store_scalar(builder, value, (scalar, is_unsigned), address, 0)
            }
            ValueSpot::Value { layout, .. } => {
                let target = ValueBuild { address, layout };
                Self::assign_whole_value(builder, ctx, target, rvalue, locals, type_ctx, expected)
            }
            ValueSpot::Column { .. } => Err(CodegenError::Internal(
                "a payload of an enum held as its bytes is written outside its construction"
                    .to_string(),
            )),
        }
    }

    /// Let a value local read in place name the bytes `rvalue` copies rather
    /// than copy them into its own slot, so each read of it loads only what
    /// it reads, from there. `false` for any other assignment, which copies.
    pub(crate) fn assign_read_in_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        (place, rvalue): (&Place, &Rvalue),
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<bool, CodegenError> {
        let Rvalue::Use(operand) = rvalue else {
            return Ok(false);
        };
        let names_source = place.projection.is_empty()
            && type_ctx.values.is_read_in_place(place.local)
            && !is_absent(operand)
            && !Self::reads_absent_placeholder(operand, type_ctx)?;
        if !names_source {
            return Ok(false);
        }
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let expected = &type_ctx.local_types[place.local.0];
        let source =
            Self::value_operand_address(builder, ctx, operand, locals, type_ctx, Some(expected))?;
        builder.def_var(*var, source);
        Ok(true)
    }

    /// Fill the bytes of the value `target` describes from `rvalue`: an
    /// aggregate builds them in place, a value read copies them. `expected` is
    /// the type the value is, which says what an enum payload read holds.
    pub(crate) fn assign_whole_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        target: ValueBuild,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected: Option<&Type>,
    ) -> Result<(), CodegenError> {
        let ValueBuild { address, layout } = target;
        match rvalue {
            Rvalue::Aggregate(AggregateKind::Struct(_) | AggregateKind::Tuple(_), operands) => {
                let build = ValueBuild { address, layout };
                Self::build_value_in_place(builder, ctx, build, operands, locals, type_ctx)
            }
            Rvalue::Aggregate(AggregateKind::Option(_), operands) => {
                let build = ValueBuild { address, layout };
                Self::build_present_optional(builder, ctx, build, operands, locals, type_ctx)
            }
            Rvalue::Aggregate(AggregateKind::Enum(enum_name, variant), operands) => {
                let build = ValueBuild { address, layout };
                let built = (enum_name.as_ref(), variant.as_ref());
                Self::build_enum_in_place(builder, ctx, build, built, operands, locals, type_ctx)
            }
            Rvalue::Use(operand) if is_absent(operand) => Self::store_absent_value(builder, target),
            Rvalue::Use(operand) if Self::reads_absent_placeholder(operand, type_ctx)? => {
                Self::cast_whole_value(builder, ctx, target, operand, locals, type_ctx)
            }
            Rvalue::Use(operand) => {
                let source =
                    Self::value_operand_address(builder, ctx, operand, locals, type_ctx, expected)?;
                copy_value(builder, source, address, layout);
                Ok(())
            }
            Rvalue::Cast(operand, _) => {
                Self::cast_whole_value(builder, ctx, target, operand, locals, type_ctx)
            }
            Rvalue::Aggregate(..)
            | Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => Err(CodegenError::Internal(format!(
                "`{rvalue}` is assigned to a value held as its bytes"
            ))),
        }
    }

    /// Store each operand of an aggregate into its member of the value being
    /// built, at the offset the value's layout gives it.
    fn build_value_in_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        build: ValueBuild,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if operands.len() != build.layout.fields.len() {
            return Err(CodegenError::Internal(format!(
                "an aggregate carries {} values for a value of {} fields",
                operands.len(),
                build.layout.fields.len()
            )));
        }
        for (operand, field) in operands.iter().zip(&build.layout.fields) {
            Self::store_member(
                builder,
                ctx,
                build.address,
                (field.offset, &field.repr),
                operand,
                locals,
                type_ctx,
            )?;
        }
        Ok(())
    }

    /// Store `operand` into the member `repr` at `offset` of the value whose
    /// bytes start at `address`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn store_member(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        address: Value,
        (offset, repr): (u32, &FieldRepr),
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        match repr {
            FieldRepr::Scalar(scalar) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)?;
                let is_unsigned = Self::operand_is_unsigned(operand, type_ctx)?;
                Self::store_scalar(builder, value, (*scalar, is_unsigned), address, offset)
            }
            FieldRepr::Value(inner) => {
                let dest = builder.ins().iadd_imm_s(address, i64::from(offset));
                let target = ValueBuild {
                    address: dest,
                    layout: inner,
                };
                if is_absent(operand) {
                    return Self::store_absent_value(builder, target);
                }
                let source =
                    Self::value_operand_address(builder, ctx, operand, locals, type_ctx, None)?;
                copy_value(builder, source, dest, inner);
                Ok(())
            }
            FieldRepr::OptionTag => Err(CodegenError::Internal(
                "an aggregate stores into an optional's tag held as bytes".to_string(),
            )),
            FieldRepr::Overlay(_) => Err(CodegenError::Internal(
                "an aggregate stores into an enum's payload column without its variant".to_string(),
            )),
        }
    }

    /// Store `value` as a scalar of `scalar` at `offset` from `address`,
    /// converting it to the scalar's width first: widened by its own sign,
    /// unsigned when `from_unsigned` says so.
    pub(super) fn store_scalar(
        builder: &mut FunctionBuilder,
        value: Value,
        (scalar, from_unsigned): (ScalarKind, bool),
        address: Value,
        offset: u32,
    ) -> Result<(), CodegenError> {
        let from = builder.func.dfg.value_type(value);
        let to = scalar_type(scalar);
        let is_unsigned = from_unsigned;
        let value = Self::cast_value_with_sign(builder, value, from, to, is_unsigned)?;
        let offset = i32::try_from(offset).map_err(|_| {
            CodegenError::Internal(format!("value member offset {offset} overflows"))
        })?;
        builder
            .ins()
            .store(MemFlagsData::new(), value, address, offset);
        Ok(())
    }
}

/// The layout of a value of `kind` written in `body`, when the body holds it
/// as its bytes (see [`Body::value_layout`]).
pub fn value_layout(body: &Body, kind: &TypeKind) -> Option<ValueLayout> {
    body.value_layout(kind)
}

/// Which locals of the body being translated are value locals, with each
/// one's layout, and the classifier any other type is judged by.
pub struct ValueLocals<'b> {
    body: Option<&'b Body>,
    layouts: Vec<Option<ValueLayout>>,
    result: Option<HeldResult>,
    /// The value locals that name the bytes they were copied from rather
    /// than a copy of their own (see [`copies_read_in_place`]).
    read_in_place: HashSet<Local>,
}

impl<'b> ValueLocals<'b> {
    /// The value locals of `body`.
    pub fn of(body: &'b Body) -> Result<Self, CodegenError> {
        let layouts: Vec<Option<ValueLayout>> = body
            .local_decls
            .iter()
            .map(|decl| value_layout(body, &decl.ty.kind))
            .collect();
        let result = layouts.first().and_then(Option::as_ref).map(HeldResult::of);
        Ok(Self {
            body: Some(body),
            layouts,
            result,
            read_in_place: copies_read_in_place(body),
        })
    }

    /// No value locals, and no type held as its bytes: the code being
    /// translated is a thunk with no body of its own.
    pub fn none() -> Self {
        Self {
            body: None,
            layouts: Vec::new(),
            result: None,
            read_in_place: HashSet::new(),
        }
    }

    /// The body whose locals these are, when the code being translated has
    /// one.
    pub fn body(&self) -> Option<&'b Body> {
        self.body
    }

    /// How the body's result `_0` comes back when it is held as its bytes,
    /// decided once from its layout: the entry binding, the frame slot and
    /// every return read this one answer. `None` for any other result.
    pub fn held_result(&self) -> Option<HeldResult> {
        self.result
    }

    /// Whether the value local `local` names the bytes it was copied from,
    /// which every read of it finds unchanged, instead of a copy.
    pub fn is_read_in_place(&self, local: Local) -> bool {
        self.read_in_place.contains(&local)
    }

    /// The layout of `local`, when it is a value local.
    pub fn layout(&self, local: Local) -> Option<&ValueLayout> {
        self.layouts.get(local.0)?.as_ref()
    }

    /// The layout of a value of `kind`, when the code being translated holds
    /// one as its bytes.
    pub fn type_layout(&self, kind: &TypeKind) -> Option<ValueLayout> {
        value_layout(self.body?, kind)
    }

    /// Where `place` reaches inside a value local's bytes, or `None` when it
    /// is not rooted in a value local.
    pub fn spot(&self, place: &Place) -> Result<Option<ValueSpot<'_>>, CodegenError> {
        self.layout(place.local)
            .map(|layout| locate(layout, &place.projection))
            .transpose()
    }

    /// The layout of the whole value `operand` reads, when it reads one held
    /// as its bytes; a scalar read out of one is a word.
    pub fn operand_value(&self, operand: &Operand) -> Result<Option<&ValueLayout>, CodegenError> {
        let (Operand::Copy(place) | Operand::Move(place)) = operand else {
            return Ok(None);
        };
        Ok(self.spot(place)?.and_then(|spot| spot.value()))
    }
}

/// What a place rooted in a value local reaches inside its bytes.
#[derive(Debug, Clone, Copy)]
pub enum ValueSpot<'l> {
    /// A scalar at `offset`.
    Scalar { offset: u32, scalar: ScalarKind },
    /// A whole value at `offset`: the local itself or a value nested in it.
    Value {
        offset: u32,
        layout: &'l ValueLayout,
    },
    /// An enum payload column at `offset` whose variants store different
    /// `members`: which one it holds is the reader's type to say.
    Column {
        offset: u32,
        members: &'l [FieldRepr],
    },
}

impl<'l> ValueSpot<'l> {
    fn offset(self) -> u32 {
        match self {
            ValueSpot::Scalar { offset, .. }
            | ValueSpot::Value { offset, .. }
            | ValueSpot::Column { offset, .. } => offset,
        }
    }

    /// The layout of the value reached, when it is a whole value.
    pub fn value(self) -> Option<&'l ValueLayout> {
        match self {
            ValueSpot::Value { layout, .. } => Some(layout),
            ValueSpot::Scalar { .. } | ValueSpot::Column { .. } => None,
        }
    }

    /// What `repr` at `offset` is reached as. An enum column whose variants
    /// all store one member is that member.
    fn of(offset: u32, repr: &'l FieldRepr, elem: &PlaceElem) -> Result<Self, CodegenError> {
        match repr {
            FieldRepr::Scalar(scalar) => Ok(ValueSpot::Scalar {
                offset,
                scalar: *scalar,
            }),
            FieldRepr::Value(layout) => Ok(ValueSpot::Value { offset, layout }),
            FieldRepr::OptionTag => Err(projection_error(elem, "an optional's bytes")),
            FieldRepr::Overlay(members) => match members.as_slice() {
                [sole] => Self::of(offset, sole, elem),
                members => Ok(ValueSpot::Column { offset, members }),
            },
        }
    }
}

/// Where `projection` reaches inside a value laid out as `layout`.
///
/// Field projections move inside a value, and an index picks an element of a
/// tuple whose elements are laid out alike; a value holds no pointer to
/// dereference, and an optional's bytes are not projected into here.
fn locate<'l>(
    layout: &'l ValueLayout,
    projection: &[PlaceElem],
) -> Result<ValueSpot<'l>, CodegenError> {
    let mut spot = ValueSpot::Value { offset: 0, layout };
    for elem in projection {
        let (offset, outer) = match spot {
            ValueSpot::Value { offset, layout } => (offset, layout),
            ValueSpot::Column { offset, members } => {
                (offset, projected_column_value(members, elem)?)
            }
            ValueSpot::Scalar { .. } => return Err(projection_error(elem, "a scalar")),
        };
        let (field_offset, repr) = match elem {
            PlaceElem::Field(index) => {
                let field = member(outer, *index)?;
                (field.offset, &field.repr)
            }
            // An element picked by a value has no offset known here; only
            // what it holds is.
            PlaceElem::Index(_) => (0, uniform_element(outer)?.1),
            PlaceElem::Deref => return Err(projection_error(elem, "a value held as its bytes")),
        };
        spot = ValueSpot::of(offset + field_offset, repr, elem)?;
    }
    Ok(spot)
}

/// What a projection inside a value held as its bytes has reached.
#[derive(Clone, Copy)]
enum ReachedRepr<'l> {
    Scalar(ScalarKind),
    Value(&'l ValueLayout),
}

impl<'l> ReachedRepr<'l> {
    fn of(repr: &'l FieldRepr, elem: &PlaceElem) -> Result<Self, CodegenError> {
        match ValueSpot::of(0, repr, elem)? {
            ValueSpot::Scalar { scalar, .. } => Ok(Self::Scalar(scalar)),
            ValueSpot::Value { layout, .. } => Ok(Self::Value(layout)),
            ValueSpot::Column { .. } => Err(projection_error(
                elem,
                "an enum payload column reached through an element picked by a value",
            )),
        }
    }
}

/// The stride between the elements of a tuple held as its bytes, and what
/// each element holds; refused unless every element is laid out alike.
fn uniform_element(layout: &ValueLayout) -> Result<(u32, &FieldRepr), CodegenError> {
    let first = layout
        .fields
        .first()
        .ok_or_else(|| CodegenError::Internal("an empty value is indexed".to_string()))?;
    let stride = match layout.fields.get(1) {
        Some(second) => second.offset - first.offset,
        None => layout.size,
    };
    let is_uniform = layout.fields.iter().enumerate().all(|(position, field)| {
        field.repr == first.repr && field.offset == first.offset + stride * position as u32
    });
    if !is_uniform {
        return Err(CodegenError::Internal(
            "a value whose elements are laid out apart is indexed by a value".to_string(),
        ));
    }
    Ok((stride, &first.repr))
}

/// Member `index` of the value laid out as `layout`, as a field projection
/// numbers it: an optional's one member is its payload.
fn member(layout: &ValueLayout, index: usize) -> Result<&ValueField, CodegenError> {
    layout.projected_field(index).ok_or_else(|| {
        CodegenError::Internal(format!(
            "field {index} is projected out of a value of {} fields",
            layout.fields.len()
        ))
    })
}

fn projection_error(elem: &PlaceElem, held: &str) -> CodegenError {
    CodegenError::Internal(format!("`{elem:?}` is projected out of {held}"))
}

/// The Cranelift type a scalar of `scalar` is loaded and stored at.
pub fn scalar_type(scalar: ScalarKind) -> cl_types::Type {
    match scalar {
        ScalarKind::Boolean => cl_types::I8,
        ScalarKind::Float(2) => cl_types::F16,
        ScalarKind::Float(4) => cl_types::F32,
        ScalarKind::Float(_) => cl_types::F64,
        ScalarKind::Signed(bytes) | ScalarKind::Unsigned(bytes) => {
            cl_types::Type::int_with_byte_size(bytes as u16).unwrap_or(cl_types::I64)
        }
    }
}

/// A stack slot of this frame for a value laid out as `layout`, and its
/// address.
pub fn value_slot(
    builder: &mut FunctionBuilder,
    layout: &ValueLayout,
    ptr_type: cl_types::Type,
) -> Value {
    let align_shift = layout.align.max(1).trailing_zeros() as u8;
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        layout.size,
        align_shift,
    ));
    builder.ins().stack_addr(ptr_type, slot, 0)
}

/// Copy `chunks` — each a byte offset and the type moved there — from the
/// bytes at `source` to the bytes at `dest`, one load and one store each.
pub fn copy_chunks(
    builder: &mut FunctionBuilder,
    source: Value,
    dest: Value,
    chunks: impl IntoIterator<Item = (i32, cl_types::Type)>,
) {
    for (offset, chunk) in chunks {
        let word = builder
            .ins()
            .load(chunk, MemFlagsData::new(), source, offset);
        builder.ins().store(MemFlagsData::new(), word, dest, offset);
    }
}

/// Copy the value laid out as `layout` from `source` to `dest`, each member
/// loaded and stored whole at its own type and offset; padding is left
/// alone.
///
/// Every access of a value's bytes — building it, reading a member, testing
/// an optional's tag — takes a member at that member's type, so copying the
/// same way keeps one shape per offset, which is what lets a slot holding
/// the value be promoted to SSA values.
pub fn copy_value(builder: &mut FunctionBuilder, source: Value, dest: Value, layout: &ValueLayout) {
    let members = scalar_members(layout)
        .into_iter()
        .map(|(offset, scalar)| (offset as i32, scalar_type(scalar)));
    copy_chunks(builder, source, dest, members);
}

/// Clear every member of a value laid out as `layout` at `dest`, each stored
/// as the zero of its own type at its own offset; padding is left alone. An
/// optional's tag is among the members, and reads absent.
pub fn clear_value(builder: &mut FunctionBuilder, dest: Value, layout: &ValueLayout) {
    for (offset, scalar) in scalar_members(layout) {
        let zero = zero_scalar(builder, scalar);
        builder
            .ins()
            .store(MemFlagsData::new(), zero, dest, offset as i32);
    }
}

/// Each scalar member of a value laid out as `layout`, with its byte offset
/// from the value's start, nested values flattened in layout order. An
/// optional's tag is the one-byte unsigned scalar it is read and written as.
///
/// An enum payload column is the scalars its members flatten to when every
/// member flattens to the same ones, and otherwise the unsigned integers
/// tiling the column: those carry whichever member a variant wrote, and are
/// the one shape every copy of the column takes.
pub(crate) fn scalar_members(layout: &ValueLayout) -> Vec<(u32, ScalarKind)> {
    let mut members = Vec::with_capacity(layout.fields.len());
    collect_scalar_members(layout, 0, &mut members);
    members
}

/// Each scalar member of `repr`, with its byte offset from `repr`'s start,
/// flattened as [`scalar_members`] flattens a value's.
pub(crate) fn repr_scalar_members(repr: &FieldRepr) -> Vec<(u32, ScalarKind)> {
    let mut members = Vec::new();
    collect_repr_members(repr, 0, &mut members);
    members
}

/// Append the scalar members of `layout`, placed at `base`, to `members`.
fn collect_scalar_members(layout: &ValueLayout, base: u32, members: &mut Vec<(u32, ScalarKind)>) {
    for field in &layout.fields {
        collect_repr_members(&field.repr, base + field.offset, members);
    }
}

/// Append the scalar members of `repr`, placed at `offset`, to `members`.
fn collect_repr_members(repr: &FieldRepr, offset: u32, members: &mut Vec<(u32, ScalarKind)>) {
    match repr {
        FieldRepr::Scalar(scalar) => members.push((offset, *scalar)),
        FieldRepr::OptionTag => members.push((offset, ScalarKind::Unsigned(1))),
        FieldRepr::Value(inner) => collect_scalar_members(inner, offset, members),
        FieldRepr::Overlay(column) => {
            let (size, _) = repr.size_and_align();
            collect_column_members(column, size, offset, members);
        }
    }
}

/// The zero of `scalar`, at the type [`scalar_type`] stores it at.
pub(crate) fn zero_scalar(builder: &mut FunctionBuilder, scalar: ScalarKind) -> Value {
    match scalar {
        ScalarKind::Float(2) => builder.ins().f16const(Ieee16::with_bits(0)),
        ScalarKind::Float(4) => builder.ins().f32const(0.0),
        ScalarKind::Float(_) => builder.ins().f64const(0.0),
        ScalarKind::Signed(_) | ScalarKind::Unsigned(_) | ScalarKind::Boolean => {
            zero_integer(builder, scalar_type(scalar))
        }
    }
}

/// The zero of the integer type `ty`. A 128-bit constant is joined from two
/// 64-bit halves: `iconst` takes no immediate that wide.
fn zero_integer(builder: &mut FunctionBuilder, ty: cl_types::Type) -> Value {
    if ty != cl_types::I128 {
        return builder.ins().iconst(ty, 0);
    }
    let half = builder.ins().iconst(cl_types::I64, 0);
    builder.ins().iconcat(half, half)
}

/// A value being built in place: where its bytes start and how they are laid
/// out.
#[derive(Clone, Copy)]
pub(crate) struct ValueBuild<'l> {
    pub(crate) address: Value,
    pub(crate) layout: &'l ValueLayout,
}
