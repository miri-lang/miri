// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Thunks through which the runtime asks a container's elements about each
//! other.
//!
//! The runtime sees an element only as the bytes of its slot, so it cannot
//! order two strings or tell whether two instances of a class are equal. For an
//! element type that answers such a question itself, codegen emits a small
//! exported function taking the two element values and calling the type's own
//! method; the container registers that function's address and calls through
//! it. A List or Array sorts through `miri.T.$compare`, and a Set or Map matches
//! elements and keys through `miri.T.$equals`.

use crate::ast::types::Type;
use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::codegen::cranelift::value_abi::{miri_signature, Passing, Returning};
use crate::error::CodegenError;
use crate::mir::symbol::Symbol;
use crate::mir::type_facts::{ElementQuestion, TypeFacts};

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, Signature, Value};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::sync::Arc;

/// The Cranelift type of the answer to `question`, which is the method's
/// return type.
fn answer_type(question: ElementQuestion, ptr_type: cl_types::Type) -> cl_types::Type {
    match question {
        ElementQuestion::Compare => ptr_type,
        ElementQuestion::Equals => cl_types::I8,
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Generates the `method` thunk `(a, b)` of `type_name` for a type that
    /// answers `method`.
    ///
    /// The two element values are borrowed for the call: a callee owns none of
    /// its parameters, so the container's references survive the question.
    /// `inst_args` selects one instantiation of a generic class, so the thunk
    /// reaches the body compiled for the element's concrete type rather than the
    /// shared one written against the parameter.
    pub(crate) fn generate_element_method_thunk(
        method: ElementQuestion,
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        inst_args: Option<&[Type]>,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        if !facts.answers(type_name, method) {
            return Ok(());
        }
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        let thunk_name = Symbol::type_thunk(
            method.thunk_kind(),
            type_name,
            inst_args.unwrap_or_default(),
        )
        .link_name();

        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns
            .push(AbiParam::new(answer_type(method, ptr_type)));

        let func_id = module
            .declare_function(&thunk_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(thunk_name.clone(), e.to_string()))?;
        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // The per-instantiation body is the one a static call names; where none
        // applies the shared symbol is what the call sites name.
        let symbol = facts.element_method_symbol(type_name, method.method_name(), inst_args);
        let callee = MethodCallee {
            method,
            withheld: facts.is_withheld(&symbol),
            symbol,
            ptr_type,
            call_conv,
        };
        let mut builder_ctx = FunctionBuilderContext::new();
        emit_element_method_body(module, ctx, &mut builder_ctx, &callee)?;

        crate::codegen::cranelift::define_function(module, func_id, ctx)
            .map_err(|e| CodegenError::define_function(thunk_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }
}

/// The user method a thunk calls, and the ABI it calls it with.
struct MethodCallee {
    method: ElementQuestion,
    /// Lowering withheld the method at this instance, so no body answers it:
    /// the thunk reports that at run time instead of calling it.
    withheld: bool,
    symbol: Symbol,
    ptr_type: cl_types::Type,
    call_conv: CallConv,
}

/// Emit a thunk body: answer for a null element without calling the user's
/// method, otherwise call it on the two values.
///
/// A null element is not reachable from Miri source — a managed slot always
/// holds a value — but a container the runtime has cleared would otherwise
/// dereference one inside the user's method.
fn emit_element_method_body(
    module: &mut ObjectModule,
    ctx: &mut cranelift_codegen::Context,
    builder_ctx: &mut FunctionBuilderContext,
    callee: &MethodCallee,
) -> Result<(), CodegenError> {
    let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);
    let left = builder.block_params(entry_block)[0];
    let right = builder.block_params(entry_block)[1];

    let call_block = builder.create_block();
    let null_block = builder.create_block();
    let null = builder.ins().iconst(callee.ptr_type, 0);
    let left_is_null = builder.ins().icmp(IntCC::Equal, left, null);
    let right_is_null = builder.ins().icmp(IntCC::Equal, right, null);
    let either_is_null = builder.ins().bor(left_is_null, right_is_null);
    builder
        .ins()
        .brif(either_is_null, null_block, &[], call_block, &[]);

    builder.switch_to_block(null_block);
    builder.seal_block(null_block);
    let null_answer = null_element_answer(&mut builder, callee, [left_is_null, right_is_null]);
    builder.ins().return_(&[null_answer]);

    builder.switch_to_block(call_block);
    builder.seal_block(call_block);
    let answer = emit_user_method_call(module, &mut builder, callee, [left, right])?;
    builder.ins().return_(&[answer]);

    builder.seal_all_blocks();
    builder.finalize(module.isa().frontend_config());
    Ok(())
}

/// The answer when at least one of the two elements is null.
///
/// Ordering puts a null first — exactly `right_is_null - left_is_null` — and
/// equality holds only when both are null.
fn null_element_answer(
    builder: &mut FunctionBuilder,
    callee: &MethodCallee,
    [left_is_null, right_is_null]: [Value; 2],
) -> Value {
    match callee.method {
        ElementQuestion::Compare => {
            let left_rank = builder.ins().uextend(callee.ptr_type, left_is_null);
            let right_rank = builder.ins().uextend(callee.ptr_type, right_is_null);
            builder.ins().isub(right_rank, left_rank)
        }
        ElementQuestion::Equals => builder
            .ins()
            .icmp(IntCC::Equal, left_is_null, right_is_null),
    }
}

/// Call the user's compiled method on two element values and hand back its
/// answer. A method always takes the allocator after its declared parameters.
///
/// A method withheld at this instance has no body to call: the runtime trap
/// that reports it is called instead, and the answer after it is never read.
fn emit_user_method_call(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    callee: &MethodCallee,
    [left, right]: [Value; 2],
) -> Result<Value, CodegenError> {
    let ptr_type = callee.ptr_type;
    if callee.withheld {
        let trap = FunctionTranslator::method_not_checked_trap(module)?;
        let local_trap = module.declare_func_in_func(trap, builder.func);
        builder.ins().call(local_trap, &[]);
        return Ok(builder
            .ins()
            .iconst(answer_type(callee.method, ptr_type), 0));
    }
    let receiver_other_and_allocator = [Passing::Word(ptr_type); 3];
    let sig = miri_signature(
        callee.call_conv,
        ptr_type,
        &receiver_other_and_allocator,
        Returning::Word(answer_type(callee.method, ptr_type)),
    );

    let callee_name = callee.symbol.link_name();
    let func_id = module
        .declare_function(&callee_name, Linkage::Import, &sig)
        .map_err(|e| CodegenError::declare_function(callee_name, e.to_string()))?;
    let local_fn = module.declare_func_in_func(func_id, builder.func);
    let no_allocator = builder.ins().iconst(ptr_type, 0);
    let call = builder.ins().call(local_fn, &[left, right, no_allocator]);
    Ok(builder.inst_results(call)[0])
}
