// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Locals that hold a bitwise value as its bytes, in a stack slot of the
//! frame.
//!
//! A *value local* is one whose declared type the value-class authority calls
//! bitwise and whose body's classifier does not count. Its Cranelift
//! `Variable` holds the address of its bytes: a slot of this frame sized and
//! aligned by the value's [`ValueLayout`], the pointer to the copy a caller
//! made for a parameter, or the caller's return slot for `_0`.
//!
//! - Reading a value place yields the address of its bytes; reading a scalar
//!   inside one loads it from its offset in the layout.
//! - Assigning a value copies its bytes into the destination; an aggregate of
//!   a value type is built straight into the destination's bytes.
//! - No count is kept for a value, and its address never reaches a word slot
//!   that outlives the frame: either is refused as an internal error.
//!
//! While every bitwise value is still counted no local is a value local, and
//! none of this emits anything.

use crate::ast::types::TypeKind;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::error::CodegenError;
use crate::mir::type_facts::{
    value_class, FieldRepr, ScalarKind, ValueClass, ValueField, ValueLayout,
};
use crate::mir::{AggregateKind, Body, Local, Operand, Place, PlaceElem, Rvalue};
use cranelift_codegen::ir::{
    types as cl_types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;

/// The widest single load and store a value copy moves, in bytes.
const COPY_CHUNK_BYTES: u32 = 8;

impl<'a> FunctionTranslator<'a> {
    /// Give every value local that is neither a parameter nor the result a
    /// stack slot of this frame, and set its `Variable` to the slot's address.
    pub(crate) fn define_value_slots(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        values: &ValueLocals,
        ptr_type: cl_types::Type,
    ) {
        let owned = (body.arg_count + 1..body.local_decls.len()).map(Local);
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
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let base = builder.use_var(*var);
        let address = match spot.offset() {
            0 => base,
            offset => builder.ins().iadd_imm(base, i64::from(offset)),
        };
        Ok(Some((address, spot)))
    }

    /// Read what `spot` reaches at `address`: a scalar is loaded, a whole
    /// value is its address.
    pub(crate) fn read_value_spot(
        builder: &mut FunctionBuilder,
        address: Value,
        spot: ValueSpot,
    ) -> Value {
        match spot {
            ValueSpot::Scalar { scalar, .. } => {
                builder
                    .ins()
                    .load(scalar_type(scalar), MemFlags::new(), address, 0)
            }
            ValueSpot::Value { .. } => address,
        }
    }

    /// The address of the bytes of the whole value `operand` reads; anything
    /// else read where a value's bytes are expected is refused.
    pub(crate) fn value_operand_address(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        if type_ctx.values.operand_value(operand)?.is_none() {
            return Err(CodegenError::Internal(format!(
                "`{operand}` is read where a value's bytes are expected, but holds none"
            )));
        }
        Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)
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
    pub(crate) fn refuse_value_escape(
        rvalue: &Rvalue,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let operands: &[Operand] = match rvalue {
            Rvalue::Use(operand) => std::slice::from_ref(operand),
            Rvalue::Cast(operand, _) => std::slice::from_ref(operand.as_ref()),
            Rvalue::Aggregate(_, operands) => operands,
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
    ) -> Result<(), CodegenError> {
        match spot {
            ValueSpot::Scalar { scalar, .. } => {
                let value = Self::translate_rvalue(builder, ctx, rvalue, locals, type_ctx, None)?;
                Self::store_scalar(builder, value, scalar, address, 0)
            }
            ValueSpot::Value { layout, .. } => {
                Self::assign_whole_value(builder, ctx, address, layout, rvalue, locals, type_ctx)
            }
        }
    }

    /// Fill the bytes of a value laid out as `layout` at `address` from
    /// `rvalue`: an aggregate builds them in place, a value read copies them.
    #[allow(clippy::too_many_arguments)]
    fn assign_whole_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        address: Value,
        layout: &ValueLayout,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        match rvalue {
            Rvalue::Aggregate(AggregateKind::Struct(_) | AggregateKind::Tuple(_), operands) => {
                let build = ValueBuild { address, layout };
                Self::build_value_in_place(builder, ctx, build, operands, locals, type_ctx)
            }
            Rvalue::Use(operand) => {
                let source = Self::value_operand_address(builder, ctx, operand, locals, type_ctx)?;
                copy_value(builder, source, address, layout);
                Ok(())
            }
            Rvalue::Aggregate(..)
            | Rvalue::Cast(..)
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
                field,
                operand,
                locals,
                type_ctx,
            )?;
        }
        Ok(())
    }

    /// Store `operand` into `field` of the value whose bytes start at
    /// `address`.
    #[allow(clippy::too_many_arguments)]
    fn store_member(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        address: Value,
        field: &ValueField,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        match &field.repr {
            FieldRepr::Scalar(scalar) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)?;
                Self::store_scalar(builder, value, *scalar, address, field.offset)
            }
            FieldRepr::Value(inner) => {
                let source = Self::value_operand_address(builder, ctx, operand, locals, type_ctx)?;
                let dest = builder.ins().iadd_imm(address, i64::from(field.offset));
                copy_value(builder, source, dest, inner);
                Ok(())
            }
            FieldRepr::OptionTag => Err(CodegenError::Internal(
                "an aggregate stores into an optional's tag held as bytes".to_string(),
            )),
        }
    }

    /// Store `value` as a scalar of `scalar` at `offset` from `address`,
    /// converting it to the scalar's width first.
    fn store_scalar(
        builder: &mut FunctionBuilder,
        value: Value,
        scalar: ScalarKind,
        address: Value,
        offset: u32,
    ) -> Result<(), CodegenError> {
        let from = builder.func.dfg.value_type(value);
        let to = scalar_type(scalar);
        let is_unsigned = matches!(scalar, ScalarKind::Unsigned(_));
        let value = Self::cast_value_with_sign(builder, value, from, to, is_unsigned)?;
        let offset = i32::try_from(offset).map_err(|_| {
            CodegenError::Internal(format!("value member offset {offset} overflows"))
        })?;
        builder.ins().store(MemFlags::new(), value, address, offset);
        Ok(())
    }
}

/// The layout of a value of `kind` written in `body`, when the body holds it
/// as its bytes: the value-class authority calls it bitwise and the body's
/// classifier does not count it.
pub fn value_layout(body: &Body, kind: &TypeKind) -> Option<ValueLayout> {
    if body.is_gpu() || matches!(kind, TypeKind::Void) {
        return None;
    }
    match value_class(kind, body.layout_scope()) {
        ValueClass::Bitwise(layout) if !body.is_counted(kind) => Some(layout),
        ValueClass::Bitwise(_)
        | ValueClass::Scalar(_)
        | ValueClass::Word(_)
        | ValueClass::Managed => None,
    }
}

/// Which locals of the body being translated are value locals, with each
/// one's layout, and the classifier any other type is judged by.
pub struct ValueLocals<'b> {
    body: Option<&'b Body>,
    layouts: Vec<Option<ValueLayout>>,
}

impl<'b> ValueLocals<'b> {
    /// The value locals of `body`.
    ///
    /// An optional held as its bytes has no translation here — building one,
    /// testing it and opening it are not laid out against its bytes — so a
    /// body holding one is refused rather than miscompiled.
    pub fn of(body: &'b Body) -> Result<Self, CodegenError> {
        let layouts: Vec<Option<ValueLayout>> = body
            .local_decls
            .iter()
            .map(|decl| value_layout(body, &decl.ty.kind))
            .collect();
        let optional = body
            .local_decls
            .iter()
            .zip(&layouts)
            .find(|(decl, layout)| layout.is_some() && matches!(decl.ty.kind, TypeKind::Option(_)));
        if let Some((decl, _)) = optional {
            return Err(CodegenError::Internal(format!(
                "the optional `{}` is held as its bytes, which has no translation",
                decl.ty.kind
            )));
        }
        Ok(Self {
            body: Some(body),
            layouts,
        })
    }

    /// No value locals, and no type held as its bytes: the code being
    /// translated is a thunk with no body of its own.
    pub fn none() -> Self {
        Self {
            body: None,
            layouts: Vec::new(),
        }
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
}

impl<'l> ValueSpot<'l> {
    fn offset(self) -> u32 {
        match self {
            ValueSpot::Scalar { offset, .. } | ValueSpot::Value { offset, .. } => offset,
        }
    }

    /// The layout of the value reached, when it is a whole value.
    pub fn value(self) -> Option<&'l ValueLayout> {
        match self {
            ValueSpot::Value { layout, .. } => Some(layout),
            ValueSpot::Scalar { .. } => None,
        }
    }
}

/// Where `projection` reaches inside a value laid out as `layout`.
///
/// Only field projections move inside a value; a value holds no pointer to
/// dereference and no collection to index, and an optional's bytes are not
/// projected into here.
fn locate<'l>(
    layout: &'l ValueLayout,
    projection: &[PlaceElem],
) -> Result<ValueSpot<'l>, CodegenError> {
    let mut spot = ValueSpot::Value { offset: 0, layout };
    for elem in projection {
        let (offset, outer) = match spot {
            ValueSpot::Value { offset, layout } => (offset, layout),
            ValueSpot::Scalar { .. } => return Err(projection_error(elem, "a scalar")),
        };
        let index = match elem {
            PlaceElem::Field(index) => *index,
            PlaceElem::Deref | PlaceElem::Index(_) => {
                return Err(projection_error(elem, "a value held as its bytes"))
            }
        };
        let field = member(outer, index)?;
        spot = match &field.repr {
            FieldRepr::Scalar(scalar) => ValueSpot::Scalar {
                offset: offset + field.offset,
                scalar: *scalar,
            },
            FieldRepr::Value(inner) => ValueSpot::Value {
                offset: offset + field.offset,
                layout: inner,
            },
            FieldRepr::OptionTag => return Err(projection_error(elem, "an optional's bytes")),
        };
    }
    Ok(spot)
}

/// Member `index` of the value laid out as `layout`, refusing an optional's
/// bytes, whose members are not the fields a projection numbers.
fn member(layout: &ValueLayout, index: usize) -> Result<&ValueField, CodegenError> {
    if matches!(
        layout.fields.first().map(|field| &field.repr),
        Some(FieldRepr::OptionTag)
    ) {
        return Err(CodegenError::Internal(format!(
            "field {index} is projected out of an optional held as its bytes"
        )));
    }
    layout.fields.get(index).ok_or_else(|| {
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
        let word = builder.ins().load(chunk, MemFlags::new(), source, offset);
        builder.ins().store(MemFlags::new(), word, dest, offset);
    }
}

/// Copy the bytes of a value laid out as `layout` from `source` to `dest`,
/// in the widest chunks both its size and its alignment allow.
pub fn copy_value(builder: &mut FunctionBuilder, source: Value, dest: Value, layout: &ValueLayout) {
    let width = copy_width(layout);
    let Some(chunk) = cl_types::Type::int_with_byte_size(width as u16) else {
        return;
    };
    let chunks = (0..layout.size / width).map(|index| ((index * width) as i32, chunk));
    copy_chunks(builder, source, dest, chunks);
}

/// The widest power of two, at most [`COPY_CHUNK_BYTES`], dividing both the
/// value's size and its alignment.
fn copy_width(layout: &ValueLayout) -> u32 {
    let bound = layout.align.clamp(1, COPY_CHUNK_BYTES);
    let size_bound = 1
        << layout
            .size
            .max(1)
            .trailing_zeros()
            .min(COPY_CHUNK_BYTES.trailing_zeros());
    bound.min(size_bound)
}

/// A value being built in place: where its bytes start and how they are laid
/// out.
#[derive(Clone, Copy)]
struct ValueBuild<'l> {
    address: Value,
    layout: &'l ValueLayout,
}
