// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Closure destructor generation.
//!
//! Emits `__dtor_{lambda_name}(env_ptr)` for lambdas that capture managed
//! values, so the runtime can DecRef captures when the closure RC reaches 0
//! without needing static knowledge of capture types at the drop site.

use crate::codegen::cranelift::translator::{
    empty_module_ctx, is_capture_managed, FunctionTranslator, TypeCtx,
};
use crate::error::CodegenError;
use crate::mir::Body;
use crate::type_checker::context::TypeDefinition;

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

impl<'a> FunctionTranslator<'a> {
    /// Generates `__dtor_{lambda_name}(env_ptr)` for a lambda that has managed captures.
    ///
    /// The destructor DecRefs every managed capture stored in the closure env,
    /// enabling correct cleanup when a closure is dropped in a scope that does not
    /// have compile-time knowledge of the capture types (e.g., after being returned
    /// from a function). Called by `emit_type_drop` when the closure RC reaches 0.
    ///
    /// Closure layout:  payload[0]=fn_ptr  payload[1]=dtor_ptr  payload[2+i]=cap_i
    pub(crate) fn generate_closure_destructor(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        lambda_name: &str,
        body: &Body,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        let ptr_size = ptr_type.bytes() as i64;

        let dtor_name = format!("__dtor_{}", lambda_name);
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
            ptr_type,
            ptr_size,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(dtor_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `__dtor_{lambda_name}(env_ptr)`: DecRef every managed
    /// capture at `env_ptr + (2+i)*ptr_size`, then return.
    /// Layout: `env_ptr[0]=fn_ptr, env_ptr[1]=dtor_ptr, env_ptr[2+i]=cap_i`.
    #[allow(clippy::too_many_arguments)]
    fn emit_closure_destructor_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        body: &Body,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
        ptr_size: i64,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let env_ptr = builder.block_params(entry_block)[0];

        let mut string_literals = HashMap::new();
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
        };

        for (i, &cap_local) in body.env_capture_locals.iter().enumerate() {
            let cap_ty = &body.local_decls[cap_local.0].ty;
            if is_capture_managed(&cap_ty.kind) {
                let offset = (2 + i as i64) * ptr_size;
                let cap_ptr = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), env_ptr, offset as i32);
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
