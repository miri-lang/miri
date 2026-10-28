// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `equals` and `hash` a set or map matches a tuple or an optional held
//! as its bytes by.
//!
//! A tuple has no declaration whose synthesized equality could be named, and
//! its bytes are not always its value: a float member makes `-0.0` equal to
//! `0.0` and NaN equal to nothing, and padding between members holds
//! unspecified bytes. Such a tuple is matched through two thunks generated
//! from its layout, each handed the address of an element's bytes: one
//! compares the members as `==` does, and one hashes them consistently with
//! it. A tuple whose bytes are its value is matched by its bytes and needs
//! neither. An optional's bytes are never its value: an absent one's payload
//! bytes are not part of it, so its tag is compared and hashed first, and its
//! payload only when present. An enum's are not either: its tag is hashed,
//! then the fields of the variant the tag names.

use crate::codegen::cranelift::held_enums::{column_member, load_tag};
use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::codegen::cranelift::value_locals::scalar_type;
use crate::error::CodegenError;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::type_facts::{FieldRepr, LayoutShape, ScalarKind, ValueLayout};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::sync::Arc;

/// The multiplier mixing each member into a value's hash.
const HASH_MULTIPLIER: i64 = 0x0100_0000_01b3;

/// Generate the `equals` and `hash` thunks of the tuple whose structural
/// symbol suffix is `encoding`, laid out as `layout`.
pub(crate) fn generate_value_identity_thunks(
    module: &mut ObjectModule,
    ctx: &mut cranelift_codegen::Context,
    isa: &Arc<dyn TargetIsa>,
    encoding: &str,
    layout: &ValueLayout,
) -> Result<(), CodegenError> {
    let ptr_type = isa.pointer_type();
    let equals = Symbol::structural_thunk(ThunkKind::Equals, encoding).link_name();
    define_thunk(
        module,
        ctx,
        isa,
        (&equals, 2, cl_types::I8),
        |builder, params| {
            FunctionTranslator::held_value_equality(builder, params[0], params[1], layout, 0)
        },
    )?;
    let hash = Symbol::structural_thunk(ThunkKind::Hash, encoding).link_name();
    define_thunk(module, ctx, isa, (&hash, 1, ptr_type), |builder, params| {
        hash_value(builder, params[0], layout)
    })
}

/// Declare and define the exported thunk `name` taking `arity` addresses and
/// answering a `result`, its body built by `body` from its parameters.
fn define_thunk(
    module: &mut ObjectModule,
    ctx: &mut cranelift_codegen::Context,
    isa: &Arc<dyn TargetIsa>,
    (name, arity, result): (&str, usize, cl_types::Type),
    body: impl FnOnce(&mut FunctionBuilder, &[Value]) -> Result<Value, CodegenError>,
) -> Result<(), CodegenError> {
    let ptr_type = isa.pointer_type();
    let mut sig = Signature::new(isa.default_call_conv());
    sig.params
        .extend((0..arity).map(|_| AbiParam::new(ptr_type)));
    sig.returns.push(AbiParam::new(result));
    let func_id = module
        .declare_function(name, Linkage::Export, &sig)
        .map_err(|e| CodegenError::declare_function(name.to_string(), e.to_string()))?;
    ctx.func = cranelift_codegen::ir::Function::with_name_signature(
        cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
        sig,
    );
    let mut builder_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();
    let answer = body(&mut builder, &params)?;
    builder.ins().return_(&[answer]);
    builder.finalize(isa.frontend_config());
    crate::codegen::cranelift::define_function(module, func_id, ctx)
        .map_err(|e| CodegenError::define_function(name.to_string(), e.to_string()))?;
    ctx.clear();
    Ok(())
}

/// The hash of the value at `address`, laid out as `layout`: every scalar
/// member mixed in order, a float as the number it holds so `-0.0` and `0.0`
/// hash alike. Padding is never read.
fn hash_value(
    builder: &mut FunctionBuilder,
    address: Value,
    layout: &ValueLayout,
) -> Result<Value, CodegenError> {
    let start = builder
        .ins()
        .iconst(cl_types::I64, layout.fields.len() as i64);
    mix_members(builder, address, layout, 0, start)
}

/// `hash` with every scalar member of the value laid out as `layout` at
/// `offset` from `address` mixed into it.
fn mix_members(
    builder: &mut FunctionBuilder,
    address: Value,
    layout: &ValueLayout,
    offset: u32,
    hash: Value,
) -> Result<Value, CodegenError> {
    match &layout.shape {
        LayoutShape::Optional => mix_optional(builder, address, layout, offset, hash),
        LayoutShape::Enum { variants } => {
            let value = HeldEnum {
                address,
                layout,
                offset,
            };
            mix_enum(builder, value, variants, hash)
        }
        LayoutShape::Record => layout.fields.iter().try_fold(hash, |hash, field| {
            mix_member(builder, address, (offset + field.offset, &field.repr), hash)
        }),
    }
}

/// `hash` with the member `repr` at `at` from `address` mixed into it. A tag
/// is no member of its own: only an optional's layout holds one, mixed in with
/// its payload. An enum's payload column says nothing without the tag naming
/// its variant, and is mixed in only through its enum.
fn mix_member(
    builder: &mut FunctionBuilder,
    address: Value,
    (at, repr): (u32, &FieldRepr),
    hash: Value,
) -> Result<Value, CodegenError> {
    match repr {
        FieldRepr::Scalar(scalar) => {
            let word = scalar_word(builder, address, at, *scalar);
            Ok(mix_word(builder, hash, word))
        }
        FieldRepr::Value(inner) => mix_members(builder, address, inner, at, hash),
        FieldRepr::OptionTag => Ok(hash),
        FieldRepr::Overlay(_) => Err(CodegenError::Internal(
            "an enum's payload column is hashed apart from its tag".to_string(),
        )),
    }
}

/// `hash` with the optional laid out as `layout` at `offset` from `address`
/// mixed into it: its tag, and its payload only when it is present, so every
/// two absent optionals hash alike.
fn mix_optional(
    builder: &mut FunctionBuilder,
    address: Value,
    layout: &ValueLayout,
    offset: u32,
    hash: Value,
) -> Result<Value, CodegenError> {
    let (Some(tag), Some(payload)) = (layout.fields.first(), layout.projected_field(0)) else {
        return Ok(hash);
    };
    let presence = builder.ins().load(
        cl_types::I8,
        MemFlagsData::new(),
        address,
        (offset + tag.offset) as i32,
    );
    let tag_word = builder.ins().uextend(cl_types::I64, presence);
    let with_tag = mix_word(builder, hash, tag_word);
    let payload = (offset + payload.offset, &payload.repr);
    let with_payload = mix_member(builder, address, payload, with_tag)?;
    Ok(builder.ins().select(presence, with_payload, with_tag))
}

/// An enum held as its bytes: where they start, how they are laid out, and
/// the offset of the enum inside them.
#[derive(Clone, Copy)]
struct HeldEnum<'l> {
    address: Value,
    layout: &'l ValueLayout,
    offset: u32,
}

/// `hash` with the enum `value` mixed into it: its tag, then the fields of
/// the variant the tag names, so two values of one variant with equal fields
/// hash alike whatever the other columns hold. `variants` lists each
/// variant's members, as the enum's layout shape does.
fn mix_enum(
    builder: &mut FunctionBuilder,
    value: HeldEnum,
    variants: &[Vec<usize>],
    hash: Value,
) -> Result<Value, CodegenError> {
    let HeldEnum {
        address,
        layout,
        offset,
    } = value;
    let tag = load_tag(builder, address, layout, offset)?;
    let with_tag = mix_word(builder, hash, tag);
    let mut mixed = with_tag;
    for (discriminant, members) in variants.iter().enumerate() {
        let mut variant_hash = with_tag;
        for (position, &member) in members.iter().enumerate() {
            let (at, repr) = column_member(layout, position, member)?;
            variant_hash = mix_member(builder, address, (offset + at, repr), variant_hash)?;
        }
        let is_variant = builder
            .ins()
            .icmp_imm_s(IntCC::Equal, tag, discriminant as i64);
        mixed = builder.ins().select(is_variant, variant_hash, mixed);
    }
    Ok(mixed)
}

/// `hash` with `word` mixed into it.
fn mix_word(builder: &mut FunctionBuilder, hash: Value, word: Value) -> Value {
    let mixed = builder.ins().bxor(hash, word);
    builder.ins().imul_imm_s(mixed, HASH_MULTIPLIER)
}

/// The scalar at `offset` from `address`, as a 64-bit word equal for every
/// two scalars `==` calls equal.
fn scalar_word(
    builder: &mut FunctionBuilder,
    address: Value,
    offset: u32,
    scalar: ScalarKind,
) -> Value {
    let ty = scalar_type(scalar);
    let value = builder
        .ins()
        .load(ty, MemFlagsData::new(), address, offset as i32);
    match scalar {
        ScalarKind::Float(_) => {
            let wide = if ty == cl_types::F64 {
                value
            } else {
                builder.ins().fpromote(cl_types::F64, value)
            };
            // Adding zero turns `-0.0` into `0.0` and leaves every other
            // number as it is.
            let zero = builder.ins().f64const(0.0);
            let number = builder.ins().fadd(wide, zero);
            builder
                .ins()
                .bitcast(cl_types::I64, MemFlagsData::new(), number)
        }
        ScalarKind::Signed(16) | ScalarKind::Unsigned(16) => {
            builder.ins().ireduce(cl_types::I64, value)
        }
        ScalarKind::Signed(8) | ScalarKind::Unsigned(8) => value,
        ScalarKind::Signed(_) => builder.ins().sextend(cl_types::I64, value),
        ScalarKind::Unsigned(_) | ScalarKind::Boolean => {
            builder.ins().uextend(cl_types::I64, value)
        }
    }
}
