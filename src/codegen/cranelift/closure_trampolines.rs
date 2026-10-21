// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Closure trampolines: the functions a runtime entry point calls a Miri
//! closure through.
//!
//! The runtime knows a closure only as a pointer and an argument only as the
//! address of its bytes, so it cannot call the closure itself. The MIR hand-off
//! (see [`crate::mir::closure_abi`]) follows every closure a runtime call takes
//! with the address of a trampoline named by its shape. A trampoline takes the
//! closure, the address of each argument and the address of the result; it
//! loads the closure's code pointer, loads each argument the closure takes as
//! a word out of its address and passes the others by address, calls the
//! closure and stores what it answers through the result address.
//!
//! A trampoline counts nothing: a callee borrows its parameters, so the
//! elements the runtime shows the closure stay owned by their slots.
//!
//! Every closure a trampoline is handed with is checked here against it: the
//! signature the trampoline calls through must be the one the closure's own
//! type gives it, the signature the closure is compiled under.

use crate::ast::types::{FunctionTypeData, TypeKind};
use crate::codegen::cranelift::value_abi::{
    function_type_signature, miri_signature, Passing, Returning,
};
use crate::codegen::cranelift::value_locals::ValueLocals;
use crate::error::CodegenError;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::{Symbol, TrampolineArgument, TrampolineShape, TrampolineWord};
use crate::mir::{Body, Operand, TerminatorKind};

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The target a trampoline is emitted for.
#[derive(Clone, Copy)]
struct Target {
    ptr_type: cl_types::Type,
    call_conv: CallConv,
}

/// Emit, once each, the trampolines the runtime calls of `bodies` hand over,
/// after checking each against the closure it is handed with.
pub(crate) fn generate_closure_trampolines(
    module: &mut ObjectModule,
    ctx: &mut Context,
    isa: &Arc<dyn TargetIsa>,
    bodies: &[(&str, &Body)],
) -> Result<(), CodegenError> {
    let target = Target {
        ptr_type: isa.pointer_type(),
        call_conv: isa.default_call_conv(),
    };
    let mut shapes = BTreeSet::new();
    for (name, body) in bodies {
        collect_trampolines(name, body, target, &mut shapes)?;
    }
    let mut builder_ctx = FunctionBuilderContext::new();
    for shape in shapes {
        define_trampoline(module, ctx, &mut builder_ctx, shape, target)?;
    }
    Ok(())
}

/// Add the shape of every trampoline the runtime calls of `body`, the body of
/// `name`, hand over to `shapes`, checking each against the closure before it.
fn collect_trampolines(
    name: &str,
    body: &Body,
    target: Target,
    shapes: &mut BTreeSet<TrampolineShape>,
) -> Result<(), CodegenError> {
    let mut handed = handed_trampolines(body).peekable();
    if handed.peek().is_none() {
        return Ok(());
    }
    let values = ValueLocals::of(body)?;
    for (closure, shape) in handed {
        check_closure(body, closure, shape, &values, target).map_err(|details| {
            CodegenError::Translation {
                name: name.to_string(),
                details,
            }
        })?;
        shapes.insert(shape.clone());
    }
    Ok(())
}

/// Each trampoline a call in `body` hands over, with the closure argument
/// right before it.
fn handed_trampolines(body: &Body) -> impl Iterator<Item = (&Operand, &TrampolineShape)> {
    body.basic_blocks
        .iter()
        .filter_map(|block| {
            let Some(TerminatorKind::Call { args, .. }) =
                block.terminator.as_ref().map(|t| &t.kind)
            else {
                return None;
            };
            Some(args)
        })
        .flat_map(|args| args.windows(2))
        .filter_map(|pair| {
            let [closure, Operand::Function(trampoline)] = pair else {
                return None;
            };
            Some((closure, trampoline.symbol.trampoline_shape()?))
        })
}

/// Refuse a trampoline whose call into the closure `closure` would not be
/// made under the signature the closure's type gives it.
fn check_closure(
    body: &Body,
    closure: &Operand,
    shape: &TrampolineShape,
    values: &ValueLocals,
    target: Target,
) -> Result<(), String> {
    let function = closure_function_type(body, closure).ok_or_else(|| {
        format!("a closure trampoline follows `{closure}`, which is not a closure")
    })?;
    let leading = [Passing::Word(target.ptr_type)];
    let compiled = function_type_signature(
        &function,
        &leading,
        values,
        target.call_conv,
        target.ptr_type,
    );
    let called = closure_signature(shape, target);
    if compiled != called {
        return Err(format!(
            "the closure `{closure}` is compiled as {compiled}, but {} would call it as {called}",
            Symbol::closure_trampoline(shape.clone()).written(),
        ));
    }
    Ok(())
}

/// The function type of the closure `operand` carries.
fn closure_function_type(body: &Body, operand: &Operand) -> Option<FunctionTypeData> {
    let ty = match operand {
        Operand::Copy(place) | Operand::Move(place) => PlaceTypes::of(body).declared(place)?,
        Operand::Constant(constant) => constant.ty.clone(),
        Operand::Function(function) => function.ty.clone(),
    };
    if let TypeKind::Function(data) = ty.kind {
        Some(*data)
    } else {
        None
    }
}

/// The signature a trampoline of `shape` calls the closure under: the closure
/// first, then each argument as the trampoline hands it on.
fn closure_signature(shape: &TrampolineShape, target: Target) -> Signature {
    let params: Vec<Passing> = std::iter::once(Passing::Word(target.ptr_type))
        .chain(shape.arguments.iter().map(|argument| match argument {
            TrampolineArgument::Word(word) => Passing::Word(word_type(*word, target)),
            TrampolineArgument::Address => Passing::ByAddress,
        }))
        .collect();
    let result = shape.result.map_or(Returning::Nothing, |word| {
        Returning::Word(word_type(word, target))
    });
    miri_signature(target.call_conv, target.ptr_type, &params, result)
}

/// The Cranelift type of `word`.
fn word_type(word: TrampolineWord, target: Target) -> cl_types::Type {
    match word {
        TrampolineWord::I8 => cl_types::I8,
        TrampolineWord::I16 => cl_types::I16,
        TrampolineWord::I32 => cl_types::I32,
        TrampolineWord::I64 => cl_types::I64,
        TrampolineWord::I128 => cl_types::I128,
        TrampolineWord::F32 => cl_types::F32,
        TrampolineWord::F64 => cl_types::F64,
        TrampolineWord::Pointer => target.ptr_type,
    }
}

/// Declare and define the trampoline of `shape`: `(closure, a0, …, an,
/// result)`, every parameter a pointer, answering nothing.
fn define_trampoline(
    module: &mut ObjectModule,
    ctx: &mut Context,
    builder_ctx: &mut FunctionBuilderContext,
    shape: TrampolineShape,
    target: Target,
) -> Result<(), CodegenError> {
    let name = Symbol::closure_trampoline(shape.clone()).link_name();
    let mut sig = Signature::new(target.call_conv);
    let words = shape.arguments.len() + 2;
    sig.params
        .extend((0..words).map(|_| AbiParam::new(target.ptr_type)));
    let func_id = module
        .declare_function(&name, Linkage::Local, &sig)
        .map_err(|e| CodegenError::declare_function(name.clone(), e.to_string()))?;
    ctx.func = cranelift_codegen::ir::Function::with_name_signature(
        UserFuncName::user(0, func_id.as_u32()),
        sig,
    );
    emit_trampoline_body(ctx, builder_ctx, &shape, target)?;
    crate::codegen::cranelift::define_function(module, func_id, ctx)
        .map_err(|e| CodegenError::define_function(name, e.to_string()))?;
    ctx.clear();
    Ok(())
}

/// The body of a trampoline: load the closure's code pointer, load or forward
/// each argument, call, and store the result.
fn emit_trampoline_body(
    ctx: &mut Context,
    builder_ctx: &mut FunctionBuilderContext,
    shape: &TrampolineShape,
    target: Target,
) -> Result<(), CodegenError> {
    let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let params = builder.block_params(entry).to_vec();
    let (Some((&closure, rest)), Some(&result)) = (params.split_first(), params.last()) else {
        return Err(CodegenError::Internal(
            "a closure trampoline was declared without its closure and result".to_string(),
        ));
    };
    let code = builder
        .ins()
        .load(target.ptr_type, MemFlags::trusted(), closure, 0);
    let mut call_args = vec![closure];
    for (argument, &slot) in shape.arguments.iter().zip(rest) {
        call_args.push(match argument {
            TrampolineArgument::Word(word) => {
                builder
                    .ins()
                    .load(word_type(*word, target), MemFlags::new(), slot, 0)
            }
            TrampolineArgument::Address => slot,
        });
    }
    let sig_ref = builder.import_signature(closure_signature(shape, target));
    let call = builder.ins().call_indirect(sig_ref, code, &call_args);
    if shape.result.is_some() {
        let answer = builder.inst_results(call).first().copied().ok_or_else(|| {
            CodegenError::Internal("a closure trampoline's call answered nothing".to_string())
        })?;
        builder.ins().store(MemFlags::new(), answer, result, 0);
    }
    builder.ins().return_(&[]);
    builder.finalize();
    Ok(())
}
