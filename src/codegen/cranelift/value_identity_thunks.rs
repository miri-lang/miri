// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `equals` and `hash` a set or map matches a tuple held as its bytes by.
//!
//! A tuple has no declaration whose synthesized equality could be named, and
//! its bytes are not always its value: a float member makes `-0.0` equal to
//! `0.0` and NaN equal to nothing, and padding between members holds
//! unspecified bytes. Such a tuple is matched through two thunks generated
//! from its layout, each handed the address of an element's bytes: one
//! compares the members as `==` does, and one hashes them consistently with
//! it. A tuple whose bytes are its value is matched by its bytes and needs
//! neither.

use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::codegen::cranelift::value_locals::scalar_type;
use crate::error::CodegenError;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::type_facts::{FieldRepr, ScalarKind, ValueLayout};
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature, Value};
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
    body: impl FnOnce(&mut FunctionBuilder, &[Value]) -> Value,
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
    let answer = body(&mut builder, &params);
    builder.ins().return_(&[answer]);
    builder.finalize();
    crate::codegen::cranelift::define_function(module, func_id, ctx)
        .map_err(|e| CodegenError::define_function(name.to_string(), e.to_string()))?;
    ctx.clear();
    Ok(())
}

/// The hash of the value at `address`, laid out as `layout`: every scalar
/// member mixed in order, a float as the number it holds so `-0.0` and `0.0`
/// hash alike. Padding is never read.
fn hash_value(builder: &mut FunctionBuilder, address: Value, layout: &ValueLayout) -> Value {
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
    mut hash: Value,
) -> Value {
    for field in &layout.fields {
        let at = offset + field.offset;
        hash = match &field.repr {
            FieldRepr::Scalar(scalar) => {
                let word = scalar_word(builder, address, at, *scalar);
                let mixed = builder.ins().bxor(hash, word);
                builder.ins().imul_imm(mixed, HASH_MULTIPLIER)
            }
            FieldRepr::Value(inner) => mix_members(builder, address, inner, at, hash),
            FieldRepr::OptionTag => hash,
        };
    }
    hash
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
        .load(ty, MemFlags::new(), address, offset as i32);
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
                .bitcast(cl_types::I64, MemFlags::new(), number)
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
