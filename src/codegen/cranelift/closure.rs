// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Closure destructor generation.
//!
//! Emits `miri.$dtor.{lambda_name}(env_ptr)` for lambdas that capture managed
//! values, so the runtime can DecRef captures when the closure RC reaches 0
//! without needing static knowledge of capture types at the drop site.

use crate::ast::types::Type;
use crate::codegen::cranelift::translate_type;
use crate::codegen::cranelift::translator::{empty_module_ctx, FunctionTranslator, TypeCtx};
use crate::error::CodegenError;
use crate::mir::rc::is_word_slot_managed;
use crate::mir::symbol::Symbol;
use crate::mir::Body;
use crate::type_checker::context::TypeDefinition;

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Where each capture lives in a closure's payload, and how many bytes the
/// payload takes.
///
/// The payload starts with the function pointer and the destructor pointer,
/// one word each, and then holds each capture in a slot as wide as its value,
/// rounded up to whole words: a float keeps its own bits and a 128-bit value
/// both of its words. The store that builds the closure, the loads that open
/// its body and its destructor all place a capture here, so they cannot
/// disagree about where it is.
pub(crate) struct CaptureLayout {
    /// The byte offset of each capture from the start of the payload.
    pub(crate) offsets: Vec<i32>,
    /// The payload's size in bytes, both pointers included.
    pub(crate) payload_bytes: i64,
}

impl CaptureLayout {
    /// The layout of captures of Cranelift types `capture_types`, in order.
    pub(crate) fn new(
        capture_types: impl IntoIterator<Item = cl_types::Type>,
        ptr_type: cl_types::Type,
    ) -> Self {
        let word = i64::from(ptr_type.bytes());
        let mut next = 2 * word;
        let offsets = capture_types
            .into_iter()
            .map(|ty| {
                let offset = next;
                let bytes = i64::from(ty.bytes()).max(word);
                next += (bytes + word - 1) / word * word;
                i32::try_from(offset).unwrap_or(i32::MAX)
            })
            .collect();
        Self {
            offsets,
            payload_bytes: next,
        }
    }

    /// The layout of the captures a lambda body declares, read from their
    /// local types.
    pub(crate) fn of_body(body: &Body, ptr_type: cl_types::Type) -> Self {
        Self::new(
            body.env_capture_locals
                .iter()
                .map(|local| translate_type(&body.local_decls[local.0].ty, ptr_type)),
            ptr_type,
        )
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Generates `miri.$dtor.{lambda_name}(env_ptr)` for a lambda that has managed captures.
    ///
    /// The destructor DecRefs every managed capture stored in the closure env,
    /// enabling correct cleanup when a closure is dropped in a scope that does not
    /// have compile-time knowledge of the capture types (e.g., after being returned
    /// from a function). Called by `emit_type_drop` when the closure RC reaches 0.
    ///
    /// Closure layout: `payload[0]=fn_ptr`, `payload[1]=dtor_ptr`, then the
    /// captures where [`CaptureLayout`] places them.
    pub(crate) fn generate_closure_destructor(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        lambda_name: &str,
        body: &Body,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<Type>>>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let dtor_name = Symbol::closure_destructor(lambda_name).link_name();
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));

        let func_id = module
            .declare_function(&dtor_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(dtor_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_closure_destructor_body(
            module,
            ctx,
            &mut builder_ctx,
            body,
            type_definitions,
            generic_class_instantiations,
            ptr_type,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(dtor_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `miri.$dtor.{lambda_name}(env_ptr)`: DecRef every managed
    /// capture where [`CaptureLayout`] places it, then return.
    #[allow(clippy::too_many_arguments)]
    fn emit_closure_destructor_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        body: &Body,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<Type>>>,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let env_ptr = builder.block_params(entry_block)[0];

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            type_definitions,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
            generic_class_instantiations,
        };

        let layout = CaptureLayout::of_body(body, ptr_type);
        for (&cap_local, &offset) in body.env_capture_locals.iter().zip(&layout.offsets) {
            let cap_ty = &body.local_decls[cap_local.0].ty;
            if is_word_slot_managed(&cap_ty.kind) {
                let cap_ptr = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), env_ptr, offset);
                Self::emit_decref_value(
                    &mut builder,
                    &mut module_ctx,
                    &cap_ty.kind,
                    cap_ptr,
                    &type_ctx,
                )?;
            }
        }

        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }
}
