// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use crate::ast::expression::ExpressionKind;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{BasicBlock, Body, Local, Place, PlaceElem};
use crate::runtime_fns::rt;
use crate::type_checker::context::TypeDefinition;

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{
    AbiParam, Block, Function, InstBuilder, MemFlags, Signature, TrapCode, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

/// Translates MIR functions to Cranelift IR.
///
/// Each `FunctionTranslator` handles a single function, managing local variables,
/// basic blocks, and the translation of statements and terminators.
///
/// The translator is constructed once per function, builds the Cranelift IR via
/// [`translate`](Self::translate), and then consumed via
/// [`into_function`](Self::into_function) to yield the built [`Function`].
pub struct FunctionTranslator<'a> {
    /// The Cranelift function being built.
    func: Function,
    /// Function builder context (reusable across functions).
    builder_ctx: FunctionBuilderContext,
    /// Default calling convention for the target.
    call_conv: CallConv,
    /// Target pointer type.
    ptr_type: cl_types::Type,
    /// Borrowed references to MIR local types to avoid cloning.
    local_types: Vec<&'a Type>,
    /// Type definitions from the type checker (for layout computation).
    /// Borrowed from the backend to avoid cloning the entire HashMap per function.
    pub(crate) type_definitions: &'a HashMap<String, TypeDefinition>,
}

/// Context for module-level resources during translation.
pub(crate) struct ModuleCtx<'a> {
    pub(crate) module: &'a mut ObjectModule,
    pub(crate) string_literals: &'a mut HashMap<String, String>,
    /// Cached FuncId for libc malloc to avoid re-declaring per call site.
    pub(crate) malloc_func_id: Option<cranelift_module::FuncId>,
    /// Cached FuncId for libc free to avoid re-declaring per call site.
    pub(crate) free_func_id: Option<cranelift_module::FuncId>,
    /// Cached FuncIds for runtime collection functions.
    pub(crate) rt_array_new_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_array_free_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_array_panic_oob_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_new_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_push_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_free_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_set_elem_drop_fn_id: Option<cranelift_module::FuncId>,
    /// Cached FuncIds for runtime map functions.
    pub(crate) rt_map_new_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_map_set_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_map_free_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_map_set_val_drop_fn_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_decref_element_id: Option<cranelift_module::FuncId>,
    /// Cached FuncIds for runtime set functions.
    pub(crate) rt_set_new_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_set_add_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_set_free_id: Option<cranelift_module::FuncId>,
}

/// Context for type information during translation.
pub(crate) struct TypeCtx<'a> {
    pub(crate) local_types: &'a [&'a Type],
    pub(crate) type_definitions: &'a HashMap<String, TypeDefinition>,
    pub(crate) ptr_type: cl_types::Type,
}

impl<'a> FunctionTranslator<'a> {
    /// Create a new function translator.
    ///
    /// # Arguments
    ///
    /// * `isa` - The target instruction set architecture
    /// * `body` - The MIR body whose local types will be cached
    /// * `type_definitions` - Borrowed type definitions for layout computation
    pub fn new(
        isa: &Arc<dyn TargetIsa>,
        body: &'a Body,
        type_definitions: &'a HashMap<String, TypeDefinition>,
    ) -> Self {
        let func = Function::new();
        let builder_ctx = FunctionBuilderContext::new();
        let ptr_type = isa.pointer_type();

        // Borrow local types for fast lookup during translation (avoids cloning)
        let local_types = body.local_decls.iter().map(|d| &d.ty).collect();

        Self {
            func,
            builder_ctx,
            call_conv: isa.default_call_conv(),
            ptr_type,
            local_types,
            type_definitions,
        }
    }
    /// Translate a MIR function body to Cranelift IR.
    pub fn translate(
        &mut self,
        body: &Body,
        module: &mut ObjectModule,
        string_literals: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Build the function signature
        self.build_signature(body)?;

        // Create function builder
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        // Keep track of locals and blocks (pre-sized to avoid rehashing)
        let mut locals: HashMap<Local, Variable> = HashMap::with_capacity(body.local_decls.len());
        let mut blocks: HashMap<BasicBlock, Block> =
            HashMap::with_capacity(body.basic_blocks.len());

        // Declare all local variables
        for (idx, local_decl) in body.local_decls.iter().enumerate() {
            let local = Local(idx);
            let cl_type = translate_type(&local_decl.ty, self.ptr_type);
            let var = builder.declare_var(cl_type);

            locals.insert(local, var);
        }

        // Create all basic blocks first
        for idx in 0..body.basic_blocks.len() {
            let mir_block = BasicBlock(idx);
            let cl_block = builder.create_block();
            blocks.insert(mir_block, cl_block);

            // Entry block receives function parameters
            if idx == 0 {
                builder.append_block_params_for_function_params(cl_block);
            }
        }

        // Switch to entry block and set up parameters
        if let Some(&entry_block) = blocks.get(&BasicBlock(0)) {
            builder.switch_to_block(entry_block);

            // Assign parameters to local variables
            let params: Vec<Value> = builder.block_params(entry_block).to_vec();
            for (i, param) in params.into_iter().enumerate() {
                let local = Local(i + 1); // Parameters start at local 1
                if let Some(&var) = locals.get(&local) {
                    builder.def_var(var, param);
                }
            }
        }

        // Create contexts once — cached FuncIds persist across all blocks.
        let mut module_ctx = ModuleCtx {
            module,
            string_literals,
            malloc_func_id: None,
            free_func_id: None,
            rt_array_new_id: None,
            rt_array_free_id: None,
            rt_array_panic_oob_id: None,
            rt_list_new_id: None,
            rt_list_push_id: None,
            rt_list_free_id: None,
            rt_list_set_elem_drop_fn_id: None,
            rt_map_new_id: None,
            rt_map_set_id: None,
            rt_map_free_id: None,
            rt_map_set_val_drop_fn_id: None,
            rt_list_decref_element_id: None,
            rt_set_new_id: None,
            rt_set_add_id: None,
            rt_set_free_id: None,
        };
        let type_ctx = TypeCtx {
            local_types: &self.local_types,
            type_definitions: self.type_definitions,
            ptr_type: self.ptr_type,
        };

        // Translate each basic block
        for (idx, block_data) in body.basic_blocks.iter().enumerate() {
            let block = blocks[&BasicBlock(idx)];
            builder.switch_to_block(block);

            // For closure bodies: load captured values from env_ptr (Local 1) at the
            // START of the entry block. This must run AFTER switch_to_block so the block
            // is in Pristine (Empty) state — Cranelift disallows switching to Partial blocks.
            // env_capture_locals[i] is loaded from env_ptr + (i+1)*ptr_size.
            if idx == 0 && !body.env_capture_locals.is_empty() {
                let env_ptr_var = locals[&Local(1)];
                let env_ptr_val = builder.use_var(env_ptr_var);

                for (i, &cap_local) in body.env_capture_locals.iter().enumerate() {
                    let offset = (i + 1) as i64 * self.ptr_type.bytes() as i64;
                    let cap_ptr = builder.ins().iadd_imm(env_ptr_val, offset);
                    let cap_cl_type =
                        translate_type(&body.local_decls[cap_local.0].ty, self.ptr_type);
                    // Load as ptr_type (captures are stored as ptr-size slots),
                    // then reduce to the target type if needed.
                    let raw_val = builder
                        .ins()
                        .load(self.ptr_type, MemFlags::new(), cap_ptr, 0);
                    let cap_val = if cap_cl_type == self.ptr_type {
                        raw_val
                    } else if cap_cl_type.is_int() && cap_cl_type.bits() < self.ptr_type.bits() {
                        builder.ins().ireduce(cap_cl_type, raw_val)
                    } else {
                        raw_val
                    };
                    if let Some(&cap_var) = locals.get(&cap_local) {
                        builder.def_var(cap_var, cap_val);
                    }
                }
            }

            // Translate all statements
            for stmt in &block_data.statements {
                Self::translate_statement(&mut builder, &mut module_ctx, stmt, &locals, &type_ctx)?;
            }

            // Translate the terminator
            if let Some(ref terminator) = block_data.terminator {
                Self::translate_terminator(
                    &mut builder,
                    &mut module_ctx,
                    terminator,
                    body,
                    &locals,
                    &blocks,
                    &type_ctx,
                )?;
            }
        }

        // Seal all blocks
        builder.seal_all_blocks();

        // Finalize the function
        builder.finalize();

        Ok(())
    }
    /// Get the function signature.
    pub fn signature(&self) -> &Signature {
        &self.func.signature
    }
    /// Consume the translator and return the built function.
    pub fn into_function(self) -> Function {
        self.func
    }
    /// Build the function signature from the MIR body.
    pub(crate) fn build_signature(&mut self, body: &Body) -> Result<(), String> {
        self.func.signature.call_conv = self.call_conv;

        // Return type is local 0
        if !body.local_decls.is_empty() {
            let ret_ty = &body.local_decls[0].ty;
            if ret_ty.kind != TypeKind::Void {
                let cl_type = translate_type(ret_ty, self.ptr_type);
                self.func.signature.returns.push(AbiParam::new(cl_type));
            }
        }

        // Parameters are locals 1..=arg_count
        for i in 1..=body.arg_count {
            if i < body.local_decls.len() {
                let param_ty = &body.local_decls[i].ty;
                let cl_type = translate_type(param_ty, self.ptr_type);
                self.func.signature.params.push(AbiParam::new(cl_type));
            }
        }

        Ok(())
    }
    /// Read a value from a place.
    pub(crate) fn read_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, String> {
        let local_types = type_ctx.local_types;
        let type_definitions = type_ctx.type_definitions;
        let ptr_type = type_ctx.ptr_type;
        let var = locals
            .get(&place.local)
            .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;

        let mut value = builder.use_var(*var);

        for proj in &place.projection {
            match proj {
                PlaceElem::Deref => {
                    value = builder.ins().load(ptr_type, MemFlags::new(), value, 0);
                }
                PlaceElem::Field(idx) => {
                    let base_type = &local_types[place.local.0];
                    let (offset, field_ty) =
                        layout::field_layout(&base_type.kind, *idx, type_definitions, ptr_type);
                    value = builder.ins().load(field_ty, MemFlags::new(), value, offset);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals
                        .get(local)
                        .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                    let idx_val = builder.use_var(*idx_var);
                    let base_type = &local_types[place.local.0];
                    value = Self::translate_collection_index_read(
                        builder, ctx, value, idx_val, base_type, type_ctx,
                    )?;
                }
            }
        }

        Ok(value)
    }
    /// Cast a value between Cranelift types.
    ///
    /// When `is_unsigned` is true, integer widening uses zero-extension
    /// and float-to-int uses unsigned saturation. Defaults to signed.
    pub(crate) fn cast_value_with_sign(
        builder: &mut FunctionBuilder,
        value: Value,
        from_ty: cranelift_codegen::ir::Type,
        to_ty: cranelift_codegen::ir::Type,
        is_unsigned: bool,
    ) -> Result<Value, String> {
        if from_ty == to_ty {
            return Ok(value);
        }

        if from_ty.is_float() && to_ty.is_float() {
            if from_ty.bytes() > to_ty.bytes() {
                Ok(builder.ins().fdemote(to_ty, value))
            } else {
                Ok(builder.ins().fpromote(to_ty, value))
            }
        } else if from_ty.is_int() && to_ty.is_int() {
            if from_ty.bytes() > to_ty.bytes() {
                Ok(builder.ins().ireduce(to_ty, value))
            } else if is_unsigned {
                Ok(builder.ins().uextend(to_ty, value))
            } else {
                Ok(builder.ins().sextend(to_ty, value))
            }
        } else if from_ty.is_float() && to_ty.is_int() {
            // Saturating float-to-int avoids trapping on NaN/overflow
            if is_unsigned {
                Ok(builder.ins().fcvt_to_uint_sat(to_ty, value))
            } else {
                Ok(builder.ins().fcvt_to_sint_sat(to_ty, value))
            }
        } else if from_ty.is_int() && to_ty.is_float() {
            if is_unsigned {
                Ok(builder.ins().fcvt_from_uint(to_ty, value))
            } else {
                Ok(builder.ins().fcvt_from_sint(to_ty, value))
            }
        } else {
            Err(format!(
                "Unsupported implicit cast from {} to {}",
                from_ty, to_ty
            ))
        }
    }
    /// Assign a value to a place.
    pub(crate) fn assign_to_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        value: Value,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let local_types = type_ctx.local_types;
        let type_definitions = type_ctx.type_definitions;
        let ptr_type = type_ctx.ptr_type;
        if place.projection.is_empty() {
            let var = locals
                .get(&place.local)
                .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;
            builder.def_var(*var, value);
        } else {
            // Base is a pointer to the aggregate
            let var = locals
                .get(&place.local)
                .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;
            let mut addr = builder.use_var(*var);

            // Navigate through all but the last projection
            for proj in &place.projection[..place.projection.len() - 1] {
                match proj {
                    PlaceElem::Deref => {
                        addr = builder.ins().load(ptr_type, MemFlags::new(), addr, 0);
                    }
                    PlaceElem::Field(idx) => {
                        let base_type = &local_types[place.local.0];
                        let (offset, _) =
                            layout::field_layout(&base_type.kind, *idx, type_definitions, ptr_type);
                        addr = builder.ins().iadd_imm(addr, offset as i64);
                    }
                    PlaceElem::Index(local) => {
                        // For intermediate Index projections, we read the indexed element
                        // (this occurs in nested projections like place[i].field)
                        let idx_var = locals
                            .get(local)
                            .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                        let idx_val = builder.use_var(*idx_var);
                        let base_type = &local_types[place.local.0];
                        addr = Self::translate_collection_index_read(
                            builder, ctx, addr, idx_val, base_type, type_ctx,
                        )?;
                    }
                }
            }

            // Apply the last projection as a store
            let last_proj = place.projection.last().ok_or_else(|| {
                "assign_to_place: empty projection after non-empty check".to_string()
            })?;
            match last_proj {
                PlaceElem::Deref => {
                    builder.ins().store(MemFlags::new(), value, addr, 0);
                }
                PlaceElem::Field(idx) => {
                    let base_type = &local_types[place.local.0];
                    let (offset, _) =
                        layout::field_layout(&base_type.kind, *idx, type_definitions, ptr_type);
                    builder.ins().store(MemFlags::new(), value, addr, offset);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals
                        .get(local)
                        .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                    let idx_val = builder.use_var(*idx_var);
                    let base_type = &local_types[place.local.0];
                    Self::translate_collection_index_write(
                        builder, ctx, addr, idx_val, value, base_type, type_ctx,
                    )?;
                }
            }
        }
        Ok(())
    }
    /// Declare-and-cache a runtime function, then call it.
    ///
    /// `cache` is one of the `Option<FuncId>` fields on `ModuleCtx`, passed
    /// separately to avoid double-borrowing `ctx`.
    fn call_cached_func(
        builder: &mut FunctionBuilder,
        module: &mut ObjectModule,
        cache: &mut Option<cranelift_module::FuncId>,
        name: &str,
        param_types: &[cranelift_codegen::ir::Type],
        return_types: &[cranelift_codegen::ir::Type],
        args: &[Value],
    ) -> Result<cranelift_codegen::ir::Inst, String> {
        let func_id = match *cache {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: param_types.iter().map(|&t| AbiParam::new(t)).collect(),
                    returns: return_types.iter().map(|&t| AbiParam::new(t)).collect(),
                    call_conv: builder.func.signature.call_conv,
                };
                let id = module
                    .declare_function(name, Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare {}: {}", name, e))?;
                *cache = Some(id);
                id
            }
        };
        let local_func = module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().call(local_func, args))
    }

    // ── Runtime collection helpers ──────────────────────────────────────

    pub(crate) fn call_rt_array_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_count: Value,
        elem_size: Value,
    ) -> Result<Value, String> {
        let pt = builder.func.dfg.value_type(elem_count);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_array_new_id,
            rt::ARRAY_NEW,
            &[pt, pt],
            &[pt],
            &[elem_count, elem_size],
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_array_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_array_free_id,
            rt::ARRAY_FREE,
            &[pt],
            &[],
            &[ptr],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_list_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, String> {
        let pt = builder.func.dfg.value_type(elem_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_list_new_id,
            rt::LIST_NEW,
            &[pt],
            &[pt],
            &[elem_size],
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_list_push(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        val: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_list_push_id,
            rt::LIST_PUSH,
            &[pt, pt],
            &[],
            &[list_ptr, val],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_list_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_list_free_id,
            rt::LIST_FREE,
            &[pt],
            &[],
            &[ptr],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_map_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_size: Value,
        value_size: Value,
        key_kind: Value,
    ) -> Result<Value, String> {
        let pt = builder.func.dfg.value_type(key_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_map_new_id,
            rt::MAP_NEW,
            &[pt, pt, pt],
            &[pt],
            &[key_size, value_size, key_kind],
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_map_set(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        key: Value,
        value: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_map_set_id,
            rt::MAP_SET,
            &[pt, pt, pt],
            &[],
            &[map_ptr, key, value],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_map_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_map_free_id,
            rt::MAP_FREE,
            &[pt],
            &[],
            &[ptr],
        )?;
        Ok(())
    }

    /// Returns the address of `miri_rt_list_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_list_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, String> {
        let func_id = match ctx.rt_list_decref_element_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function(rt::LIST_DECREF_ELEMENT, Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare {}: {}", rt::LIST_DECREF_ELEMENT, e))?;
                ctx.rt_list_decref_element_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_map_set_val_drop_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_val_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_map_set_val_drop_fn_id,
            rt::MAP_SET_VAL_DROP_FN,
            &[pt, pt],
            &[],
            &[map_ptr, fn_ptr],
        )?;
        Ok(())
    }

    /// Calls `miri_rt_list_set_elem_drop_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_list_set_elem_drop_fn_id,
            rt::LIST_SET_ELEM_DROP_FN,
            &[pt, pt],
            &[],
            &[list_ptr, fn_ptr],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_set_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, String> {
        let pt = builder.func.dfg.value_type(elem_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_set_new_id,
            rt::SET_NEW,
            &[pt],
            &[pt],
            &[elem_size],
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_set_add(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        elem: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_set_add_id,
            rt::SET_ADD,
            &[pt, pt],
            &[cl_types::I8],
            &[set_ptr, elem],
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_set_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_set_free_id,
            rt::SET_FREE,
            &[pt],
            &[],
            &[ptr],
        )?;
        Ok(())
    }

    /// Widens or narrows a value to pointer type for FFI calls.
    pub(crate) fn widen_to_ptr(
        builder: &mut FunctionBuilder,
        val: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Value {
        let val_ty = builder.func.dfg.value_type(val);
        if val_ty.bytes() < ptr_type.bytes() {
            builder.ins().sextend(ptr_type, val)
        } else if val_ty.bytes() > ptr_type.bytes() {
            builder.ins().ireduce(ptr_type, val)
        } else {
            val
        }
    }

    /// Resolves the element TypeKind from a collection type (Array or List).
    /// Returns the element TypeKind and its Cranelift type.
    pub(crate) fn resolve_collection_elem_type(
        base_type: &Type,
        _ptr_type: cl_types::Type,
    ) -> &TypeKind {
        match &base_type.kind {
            TypeKind::Array(elem_ty_expr, _) | TypeKind::List(elem_ty_expr) => {
                match &elem_ty_expr.node {
                    ExpressionKind::Type(ty, _) => &ty.kind,
                    _ => &TypeKind::Int,
                }
            }
            TypeKind::Custom(name, args)
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) =>
            {
                if let Some(args) = args {
                    if let Some(arg) = args.first() {
                        match &arg.node {
                            ExpressionKind::Type(ty, _) => &ty.kind,
                            _ => &TypeKind::Int,
                        }
                    } else {
                        &TypeKind::Int
                    }
                } else {
                    &TypeKind::Int
                }
            }
            TypeKind::Tuple(elems) => {
                // For homogeneous tuples, return the element type from the first element
                if let Some(first) = elems.first() {
                    match &first.node {
                        ExpressionKind::Type(ty, _) => &ty.kind,
                        _ => &TypeKind::Int,
                    }
                } else {
                    &TypeKind::Int
                }
            }
            _ => &TypeKind::Int,
        }
    }

    /// Returns true if the given type is a List (dynamic collection).
    pub(crate) fn is_list_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::List)
    }

    /// Returns true if the type kind is an unsigned integer.
    pub(crate) fn is_unsigned_type_kind(kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        )
    }

    /// Returns true if the given type is an Array, List, Map, or Set collection.
    pub(crate) fn is_collection_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection().is_some()
    }

    /// Returns true if the given type is a Map.
    pub(crate) fn is_map_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map)
    }

    /// Returns true if the given type is a Set.
    pub(crate) fn is_set_type(kind: &TypeKind) -> bool {
        kind.as_builtin_collection() == Some(BuiltinCollectionKind::Set)
    }

    /// Emits a loop that DecRefs each managed value in a map's hash table.
    ///
    /// Iterates over all `capacity` slots, checks the state byte for SLOT_OCCUPIED (1),
    /// and DecRefs the value pointer in each occupied slot.
    ///
    /// MiriMap states array: 1 byte per slot (0=empty, 1=occupied, 2=tombstone).
    /// Values are stored as pointer-sized entries (managed types are always pointers).
    fn emit_map_managed_values_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        states: Value,
        values: Value,
        capacity: Value,
        val_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let check_block = builder.create_block();
        let decref_block = builder.create_block();
        let increment_block = builder.create_block();
        let after_loop = builder.create_block();

        // Enter loop with index 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        // Loop header: check i < capacity
        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            capacity,
        );
        builder
            .ins()
            .brif(in_range, check_block, &[], after_loop, &[]);

        // Check state[i] == SLOT_OCCUPIED (1)
        builder.switch_to_block(check_block);
        builder.seal_block(check_block);
        let state_addr = builder.ins().iadd(states, i);
        let state_i8 = builder
            .ins()
            .load(cl_types::I8, MemFlags::new(), state_addr, 0);
        let state = builder.ins().uextend(ptr_type, state_i8);
        let slot_occupied = builder.ins().iconst(ptr_type, 1);
        let is_occupied = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            state,
            slot_occupied,
        );
        builder
            .ins()
            .brif(is_occupied, decref_block, &[], increment_block, &[]);

        // DecRef the value in this slot
        builder.switch_to_block(decref_block);
        builder.seal_block(decref_block);
        let byte_offset = builder.ins().imul_imm(i, ptr_size);
        let val_addr = builder.ins().iadd(values, byte_offset);
        let val_ptr = builder.ins().load(ptr_type, MemFlags::new(), val_addr, 0);
        Self::emit_decref_value(builder, ctx, val_kind, val_ptr, type_ctx)?;
        builder.ins().jump(increment_block, &[]);

        // Increment i and loop back
        builder.switch_to_block(increment_block);
        builder.seal_block(increment_block);
        let next_i = builder.ins().iadd_imm(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        builder.seal_block(loop_header);
        builder.seal_block(after_loop);
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits a loop that DecRefs each managed element in a contiguous pointer array.
    ///
    /// Used when freeing a List or Array whose inner type is managed, so that the
    /// RC of each stored element is properly decremented before the container is freed.
    ///
    /// `data` — pointer to the element buffer (pointer-sized elements assumed).
    /// `len`  — number of elements.
    fn emit_managed_elements_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        data: Value,
        len: Value,
        inner_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let loop_body = builder.create_block();
        let after_loop = builder.create_block();

        // Jump into the loop with initial index = 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            len,
        );
        builder
            .ins()
            .brif(in_range, loop_body, &[], after_loop, &[]);

        // loop_body: load element, DecRef it, increment index
        builder.switch_to_block(loop_body);
        builder.seal_block(loop_body); // only predecessor: loop_header

        let byte_offset = builder.ins().imul_imm(i, ptr_size);
        let elem_addr = builder.ins().iadd(data, byte_offset);
        let elem_ptr = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
        Self::emit_decref_value(builder, ctx, inner_kind, elem_ptr, type_ctx)?;

        let next_i = builder.ins().iadd_imm(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        // Seal loop_header now that both predecessors are defined
        builder.seal_block(loop_header);

        builder.seal_block(after_loop); // only predecessor: loop_header
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits the type-appropriate cleanup when an object's RC reaches zero.
    ///
    /// All heap types share the same `[RC][payload]` layout, so the RC
    /// increment/decrement logic is uniform. The only type-specific part is
    /// *what* to free when RC hits zero:
    /// - Arrays/Lists call their runtime `_free` functions (which handle internal buffers).
    /// - Structs/enums with managed fields: DecRef each managed field, then free the block.
    /// - All other types just need the `[RC][payload]` block freed.
    ///
    /// `ptr` points to the payload (past the RC header).
    /// `header_ptr` points to the RC header (`ptr - ptr_size`).
    pub(crate) fn emit_type_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        // Resolve type aliases before dispatching so that e.g.
        // `type IntArray is [int; 2]` correctly frees via rt_array_free.
        let resolved = Self::resolve_alias(kind, type_ctx.type_definitions);
        let kind = resolved.unwrap_or(kind);

        if Self::is_map_type(kind) {
            // DecRef managed values before freeing the map struct.
            // MiriMap layout: [states: ptr][keys: ptr][values: ptr][len: ptr][capacity: ptr]...
            // (each field is ptr_size bytes on the target platform)
            if let TypeKind::Map(_, val_expr) = kind {
                if let ExpressionKind::Type(val_ty, _) = &val_expr.node {
                    if is_field_managed(&val_ty.kind) {
                        let ptr_type = type_ctx.ptr_type;
                        let ptr_size = ptr_type.bytes() as i32;
                        // states pointer at offset 0
                        let states = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                        // values pointer at offset 2 * ptr_size
                        let values =
                            builder
                                .ins()
                                .load(ptr_type, MemFlags::new(), ptr, 2 * ptr_size);
                        // capacity at offset 4 * ptr_size
                        let capacity =
                            builder
                                .ins()
                                .load(ptr_type, MemFlags::new(), ptr, 4 * ptr_size);
                        Self::emit_map_managed_values_drop_loop(
                            builder,
                            ctx,
                            states,
                            values,
                            capacity,
                            &val_ty.kind,
                            type_ctx,
                        )?;
                    }
                }
            }
            Self::call_rt_map_free(builder, ctx, ptr)
        } else if Self::is_set_type(kind) {
            Self::call_rt_set_free(builder, ctx, ptr)
        } else if Self::is_list_type(kind) {
            // DecRef managed elements before freeing the list struct.
            // MiriList layout: [data: ptr][len: ptr][capacity: ptr][elem_size: ptr]
            if let TypeKind::List(inner_expr) = kind {
                if let ExpressionKind::Type(inner_ty, _) = &inner_expr.node {
                    if is_field_managed(&inner_ty.kind) {
                        let ptr_type = type_ctx.ptr_type;
                        let ptr_size = ptr_type.bytes() as i32;
                        let data = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                        let len = builder.ins().load(ptr_type, MemFlags::new(), ptr, ptr_size);
                        Self::emit_managed_elements_drop_loop(
                            builder,
                            ctx,
                            data,
                            len,
                            &inner_ty.kind,
                            type_ctx,
                        )?;
                    }
                }
            }
            Self::call_rt_list_free(builder, ctx, ptr)
        } else if Self::is_collection_type(kind) {
            // DecRef managed elements before freeing the array struct.
            // MiriArray layout: [data: ptr][elem_count: ptr][elem_size: ptr]
            if let TypeKind::Array(inner_expr, _) = kind {
                if let ExpressionKind::Type(inner_ty, _) = &inner_expr.node {
                    if is_field_managed(&inner_ty.kind) {
                        let ptr_type = type_ctx.ptr_type;
                        let ptr_size = ptr_type.bytes() as i32;
                        let data = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                        let len = builder.ins().load(ptr_type, MemFlags::new(), ptr, ptr_size);
                        Self::emit_managed_elements_drop_loop(
                            builder,
                            ctx,
                            data,
                            len,
                            &inner_ty.kind,
                            type_ctx,
                        )?;
                    }
                }
            }
            Self::call_rt_array_free(builder, ctx, ptr)
        } else if let TypeKind::Tuple(element_exprs) = kind {
            // DecRef each managed field before freeing the tuple.
            // Tuple layout: [elem_count: ptr][field0][field1]... (payload_ptr = ptr)
            let ptr_type = type_ctx.ptr_type;
            let tuple_type = kind.clone();
            let managed_fields: Vec<(i32, TypeKind)> = element_exprs
                .iter()
                .enumerate()
                .filter_map(|(i, expr)| {
                    if let ExpressionKind::Type(ty, _) = &expr.node {
                        if is_field_managed(&ty.kind) {
                            let (offset, _) = layout::field_layout(
                                &tuple_type,
                                i,
                                type_ctx.type_definitions,
                                ptr_type,
                            );
                            Some((offset, ty.kind.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            for (offset, field_kind) in managed_fields {
                let field_ptr = builder.ins().load(ptr_type, MemFlags::new(), ptr, offset);
                Self::emit_decref_value(builder, ctx, &field_kind, field_ptr, type_ctx)?;
            }
            Self::call_libc_free(builder, ctx, header_ptr)
        } else if let TypeKind::Option(inner) = kind {
            // Drop specialization: DecRef the inner value if it's managed, then free the Option space.
            if is_field_managed(&inner.kind) {
                let ptr_type = type_ctx.ptr_type;
                let cl_inner_ty =
                    crate::codegen::cranelift::types::translate_type_kind(&inner.kind, ptr_type);
                let inner_ptr =
                    builder
                        .ins()
                        .load(cl_inner_ty, cranelift_codegen::ir::MemFlags::new(), ptr, 0);
                Self::emit_decref_value(builder, ctx, &inner.kind, inner_ptr, type_ctx)?;
            }
            Self::call_libc_free(builder, ctx, header_ptr)
        } else if let TypeKind::Custom(name, _) = kind {
            // Dispatch through the type-specific drop function, which encapsulates:
            // (1) user-defined drop hook — no-op placeholder until M5 Task 3,
            // (2) recursive DecRef of all managed fields,
            // (3) freeing the RC allocation.
            //
            // For types with no managed fields we skip the thunk and free directly,
            // since their drop function would be a no-op aside from the free.
            if Self::has_managed_fields(name, type_ctx.type_definitions) {
                Self::call_drop_thunk(builder, ctx, name, ptr, type_ctx.ptr_type)
            } else {
                Self::call_libc_free(builder, ctx, header_ptr)
            }
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// Resolves a type alias to its underlying type kind.
    /// Returns `Some(resolved_kind)` if the type is an alias, `None` otherwise.
    fn resolve_alias<'b>(
        kind: &TypeKind,
        type_definitions: &'b HashMap<String, TypeDefinition>,
    ) -> Option<&'b TypeKind> {
        if let TypeKind::Custom(name, _) = kind {
            if let Some(TypeDefinition::Alias(alias_def)) = type_definitions.get(name) {
                // Recurse to handle chained aliases (A -> B -> [int])
                let inner = &alias_def.template.kind;
                Self::resolve_alias(inner, type_definitions).or(Some(inner))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Emits DecRef calls for all managed fields of a struct, class, or enum.
    ///
    /// For structs and classes, iterates all fields and emits a DecRef sequence
    /// for each managed (heap-allocated) field. For enums, reads the discriminant
    /// and conditionally DecRefs the active variant's managed fields.
    ///
    /// This is the body of the generated `__drop_TypeName` function and is also
    /// called directly from `generate_drop_function`.
    pub(crate) fn emit_struct_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;

        let Some(def) = type_ctx.type_definitions.get(type_name) else {
            return Ok(());
        };

        match def {
            TypeDefinition::Struct(struct_def) => {
                // Collect field info upfront to avoid borrowing type_ctx across builder calls.
                let managed_fields: Vec<(usize, TypeKind)> = struct_def
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, ty, _))| is_field_managed(&ty.kind))
                    .map(|(idx, (_, ty, _))| (idx, ty.kind.clone()))
                    .collect();

                let custom_kind = TypeKind::Custom(type_name.to_string(), None);
                for (field_idx, field_kind) in &managed_fields {
                    let (offset, _cl_ty) = layout::field_layout(
                        &custom_kind,
                        *field_idx,
                        type_ctx.type_definitions,
                        ptr_type,
                    );
                    let field_ptr =
                        builder
                            .ins()
                            .load(ptr_type, MemFlags::new(), payload_ptr, offset);
                    Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
                }
            }
            TypeDefinition::Enum(enum_def) => {
                // Read discriminant at offset 0
                let disc = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), payload_ptr, 0);
                let ptr_size = ptr_type.bytes() as i32;

                // Collect variant info to avoid borrow conflicts.
                let variants_with_managed: Vec<(usize, Vec<(usize, TypeKind)>)> = enum_def
                    .variants
                    .iter()
                    .enumerate()
                    .filter_map(|(vi, (_name, fields))| {
                        let managed: Vec<(usize, TypeKind)> = fields
                            .iter()
                            .enumerate()
                            .filter(|(_, ty)| is_field_managed(&ty.kind))
                            .map(|(fi, ty)| (fi, ty.kind.clone()))
                            .collect();
                        if managed.is_empty() {
                            None
                        } else {
                            Some((vi, managed))
                        }
                    })
                    .collect();

                for (variant_idx, managed_fields) in &variants_with_managed {
                    let variant_val = builder.ins().iconst(ptr_type, *variant_idx as i64);
                    let is_this_variant = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        disc,
                        variant_val,
                    );

                    let drop_block = builder.create_block();
                    let merge_block = builder.create_block();
                    builder
                        .ins()
                        .brif(is_this_variant, drop_block, &[], merge_block, &[]);

                    builder.switch_to_block(drop_block);
                    for (field_idx, field_kind) in managed_fields {
                        let field_offset = ptr_size + (*field_idx as i32 * ptr_size);
                        let field_ptr = builder.ins().load(
                            ptr_type,
                            MemFlags::new(),
                            payload_ptr,
                            field_offset,
                        );
                        Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
                    }
                    builder.ins().jump(merge_block, &[]);
                    builder.seal_block(drop_block);
                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                }
            }
            TypeDefinition::Class(class_def) => {
                // Classes use the same field-by-field drop logic as structs.
                // Fields are stored in declaration order using field_layout().
                //
                // IMPORTANT: field_layout() expects an index into the FULL field
                // list (inherited + own fields via collect_class_fields_all).
                // We must enumerate all_fields here so the indices match.
                use crate::type_checker::context::collect_class_fields_all;
                let all_fields = collect_class_fields_all(class_def, type_ctx.type_definitions);
                let managed_fields: Vec<(usize, TypeKind)> = all_fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, fi))| is_field_managed(&fi.ty.kind))
                    .map(|(idx, (_, fi))| (idx, fi.ty.kind.clone()))
                    .collect();

                let custom_kind = TypeKind::Custom(type_name.to_string(), None);
                for (field_idx, field_kind) in &managed_fields {
                    let (offset, _cl_ty) = layout::field_layout(
                        &custom_kind,
                        *field_idx,
                        type_ctx.type_definitions,
                        ptr_type,
                    );
                    let field_ptr =
                        builder
                            .ins()
                            .load(ptr_type, MemFlags::new(), payload_ptr, offset);
                    Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
                }
            }
            TypeDefinition::Trait(_) | TypeDefinition::Alias(_) | TypeDefinition::Generic(_) => {}
        }

        Ok(())
    }

    /// Emits an inline DecRef sequence for a managed value.
    ///
    /// Checks the RC header, decrements it, and if zero calls emit_type_drop
    /// recursively. This is the same logic as `StatementKind::DecRef` but for
    /// an arbitrary `Value` (not tied to a MIR local).
    fn emit_decref_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        // Guard: skip if pointer is null
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        builder.switch_to_block(rc_block);

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        let rc = builder.ins().load(
            ptr_type,
            cranelift_codegen::ir::MemFlags::new(),
            header_ptr,
            0,
        );

        // Skip immortal objects (RC < 0)
        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        builder.switch_to_block(dec_block);
        let new_rc = builder.ins().iadd_imm(rc, -1);
        builder.ins().store(
            cranelift_codegen::ir::MemFlags::new(),
            new_rc,
            header_ptr,
            0,
        );

        let zero = builder.ins().iconst(ptr_type, 0);
        let is_zero =
            builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, new_rc, zero);

        let free_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, free_block, &[], merge_block, &[]);

        builder.switch_to_block(free_block);
        Self::emit_type_drop(builder, ctx, kind, ptr, header_ptr, type_ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(rc_block);
        builder.seal_block(dec_block);
        builder.seal_block(free_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

        Ok(())
    }

    pub(crate) fn call_rt_array_panic_oob(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        index: Value,
        len: Value,
    ) -> Result<(), String> {
        let pt = builder.func.dfg.value_type(index);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.rt_array_panic_oob_id,
            rt::ARRAY_PANIC_OOB,
            &[pt, pt],
            &[],
            &[index, len],
        )?;
        Ok(())
    }

    /// Translates an index access on a collection (Array or List).
    ///
    /// Both `MiriArray` and `MiriList` have compatible layout for the first two fields:
    /// - offset 0: `data: *mut u8` (pointer to element storage)
    /// - offset ptr_size: `elem_count/len: usize`
    ///
    /// This method:
    /// 1. Reads the data pointer from the struct
    /// 2. Reads the length for bounds checking
    /// 3. Emits a trap on out-of-bounds
    /// 4. Computes the element address: `data + index * elem_size`
    pub(crate) fn translate_collection_index_read(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base_value: Value,
        idx_val: Value,
        base_type: &Type,
        type_ctx: &TypeCtx,
    ) -> Result<Value, String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        // Tuples: layout is [count][field0][field1]... at payload_ptr.
        // Fields start at offset ptr_size (after count).
        // For homogeneous tuples, element_at(i) = base + ptr_size + i * elem_size.
        // Also handle Custom("Tuple", ...) from inside the class body.
        let is_tuple_type = matches!(&base_type.kind, TypeKind::Tuple(_))
            || matches!(&base_type.kind, TypeKind::Custom(name, _) if name == "Tuple");

        if is_tuple_type {
            let elem_type_kind = Self::resolve_collection_elem_type(base_type, ptr_type);
            let cl_elem_ty =
                crate::codegen::cranelift::types::translate_type_kind(elem_type_kind, ptr_type);
            let elem_size = cl_elem_ty.bytes() as i64;

            // Read length from offset 0 (stored count)
            let len_val = builder.ins().load(ptr_type, MemFlags::new(), base_value, 0);

            // Runtime bounds check
            let oob = builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
                idx_val,
                len_val,
            );
            let panic_block = builder.create_block();
            let cont_block = builder.create_block();
            builder.ins().brif(oob, panic_block, &[], cont_block, &[]);

            builder.switch_to_block(panic_block);
            Self::call_rt_array_panic_oob(builder, ctx, idx_val, len_val)?;
            builder.ins().trap(TrapCode::unwrap_user(1));

            builder.switch_to_block(cont_block);

            // Compute element address: base + ptr_size + index * elem_size
            // Fields start after the count header
            let fields_base = builder.ins().iadd_imm(base_value, ptr_size as i64);
            let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
            let byte_offset = builder.ins().imul(idx_val, elem_size_val);
            let elem_addr = builder.ins().iadd(fields_base, byte_offset);

            builder.seal_block(panic_block);
            builder.seal_block(cont_block);

            return Ok(builder
                .ins()
                .load(cl_elem_ty, MemFlags::new(), elem_addr, 0));
        }

        let elem_type_kind = Self::resolve_collection_elem_type(base_type, ptr_type);
        let cl_elem_ty =
            crate::codegen::cranelift::types::translate_type_kind(elem_type_kind, ptr_type);
        let elem_size = cl_elem_ty.bytes() as i64;

        // MiriArray/MiriList layout: { data: *mut u8, len: usize, ... }
        // Read data pointer from offset 0
        let data_ptr = builder.ins().load(ptr_type, MemFlags::new(), base_value, 0);

        // Read length from offset ptr_size
        let len_val = builder
            .ins()
            .load(ptr_type, MemFlags::new(), base_value, ptr_size);

        // Runtime bounds check
        let oob = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx_val,
            len_val,
        );
        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(oob, panic_block, &[], cont_block, &[]);

        builder.switch_to_block(panic_block);
        Self::call_rt_array_panic_oob(builder, ctx, idx_val, len_val)?;
        builder.ins().trap(TrapCode::unwrap_user(1));

        builder.switch_to_block(cont_block);

        // Compute element address: data + index * elem_size
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);

        // Seal blocks
        builder.seal_block(panic_block);
        builder.seal_block(cont_block);

        // Load the element
        Ok(builder
            .ins()
            .load(cl_elem_ty, MemFlags::new(), elem_addr, 0))
    }

    /// Translates an index write on a collection (Array or List).
    /// Same layout assumptions as `translate_collection_index_read`.
    pub(crate) fn translate_collection_index_write(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base_addr: Value,
        idx_val: Value,
        value: Value,
        base_type: &Type,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        let elem_type_kind = Self::resolve_collection_elem_type(base_type, ptr_type);
        let cl_elem_ty =
            crate::codegen::cranelift::types::translate_type_kind(elem_type_kind, ptr_type);
        let elem_size = cl_elem_ty.bytes() as i64;

        // Read data pointer from offset 0
        let data_ptr = builder.ins().load(ptr_type, MemFlags::new(), base_addr, 0);

        // Read length from offset ptr_size
        let len_val = builder
            .ins()
            .load(ptr_type, MemFlags::new(), base_addr, ptr_size);

        // Runtime bounds check
        let oob = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx_val,
            len_val,
        );

        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(oob, panic_block, &[], cont_block, &[]);

        builder.switch_to_block(panic_block);
        Self::call_rt_array_panic_oob(builder, ctx, idx_val, len_val)?;
        builder.ins().trap(TrapCode::unwrap_user(1));

        builder.switch_to_block(cont_block);

        // Compute element address and store
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);
        builder.ins().store(MemFlags::new(), value, elem_addr, 0);

        builder.seal_block(panic_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Emits a call to the type-specific drop thunk `__drop_{type_name}(ptr)`.
    ///
    /// This is the sole call site for dropping a Custom type once RC reaches zero.
    /// The thunk function is declared as `Import` here and must be defined elsewhere
    /// (via `generate_drop_function`) before the final link.
    fn call_drop_thunk(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), String> {
        let thunk_name = format!("__drop_{}", type_name);
        let mut sig = Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = ctx
            .module
            .declare_function(&thunk_name, Linkage::Import, &sig)
            .map_err(|e| format!("Failed to declare drop thunk {thunk_name}: {e}"))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
    }

    /// Generates the `__drop_{type_name}(ptr)` function in the given module.
    ///
    /// The generated function implements the three-step destructor pipeline:
    /// 1. User-defined drop hook — no-op placeholder (M5 Task 3 will add this).
    /// 2. Recursively DecRef all managed fields.
    /// 3. Free the RC allocation via `libc::free`.
    ///
    /// This function is called once per managed concrete type during codegen,
    /// before any user functions are compiled, so the thunk symbols are available
    /// when user code later references them via Import declarations.
    pub(crate) fn generate_drop_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), String> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        let ptr_size = ptr_type.bytes() as i64;

        // Declare the function with Export linkage so other functions can call it.
        let func_name = format!("__drop_{}", type_name);
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&func_name, Linkage::Export, &sig)
            .map_err(|e| format!("Failed to declare {func_name}: {e}"))?;

        // Build the function IR.
        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let ptr = builder.block_params(entry_block)[0];

            let mut string_literals = HashMap::new();
            let mut module_ctx = ModuleCtx {
                module,
                string_literals: &mut string_literals,
                malloc_func_id: None,
                free_func_id: None,
                rt_array_new_id: None,
                rt_array_free_id: None,
                rt_array_panic_oob_id: None,
                rt_list_new_id: None,
                rt_list_push_id: None,
                rt_list_free_id: None,
                rt_list_set_elem_drop_fn_id: None,
                rt_map_new_id: None,
                rt_map_set_id: None,
                rt_map_free_id: None,
                rt_map_set_val_drop_fn_id: None,
                rt_list_decref_element_id: None,
                rt_set_new_id: None,
                rt_set_add_id: None,
                rt_set_free_id: None,
            };
            let type_ctx = TypeCtx {
                local_types: &[],
                type_definitions,
                ptr_type,
            };

            // Step 1: User-defined drop hook (no-op placeholder for M5 Task 3).

            // Step 2: DecRef all managed fields.
            Self::emit_struct_drop(&mut builder, &mut module_ctx, type_name, ptr, &type_ctx)?;

            // Step 3: Free the RC allocation.
            // header_ptr = ptr - ptr_size (points to the RC word).
            let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
            Self::call_libc_free(&mut builder, &mut module_ctx, header_ptr)?;

            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize();
        }

        module
            .define_function(func_id, ctx)
            .map_err(|e| format!("Failed to define {func_name}: {e}"))?;

        ctx.clear();
        Ok(())
    }

    /// Helper to call libc malloc, caching the FuncId across invocations.
    pub(crate) fn call_libc_malloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        size: Value,
    ) -> Result<Value, String> {
        let pt = builder.func.dfg.value_type(size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.malloc_func_id,
            "malloc",
            &[pt],
            &[pt],
            &[size],
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    /// Helper to call libc free, caching the FuncId across invocations.
    ///
    /// `header_ptr` points to the RC header (payload - ptr_size). The real
    /// malloc pointer is stored at (header_ptr - ptr_size) by `Rvalue::Allocate`,
    /// and is loaded here so that `free()` receives the original allocation.
    pub(crate) fn call_libc_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        header_ptr: Value,
    ) -> Result<(), String> {
        let ptr_type = builder.func.dfg.value_type(header_ptr);
        let ptr_size = ptr_type.bytes() as i64;

        // The real malloc pointer is stored at (header_ptr - ptr_size).
        let malloc_ptr_slot = builder.ins().iadd_imm(header_ptr, -ptr_size);
        let real_ptr = builder
            .ins()
            .load(ptr_type, MemFlags::new(), malloc_ptr_slot, 0);

        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.free_func_id,
            "free",
            &[ptr_type],
            &[],
            &[real_ptr],
        )?;
        Ok(())
    }

    /// Generate `__vtable_ClassName` static data for each concrete class that
    /// participates in virtual dispatch (has an abstract class in its hierarchy).
    ///
    /// The vtable is an array of function pointers in alphabetical order of the
    /// abstract interface class's non-constructor methods. Each slot points to the
    /// concrete implementation resolved from the class's inheritance chain.
    ///
    /// Must be called AFTER all user function bodies are compiled, so the function
    /// symbols are registered in the module.
    pub(crate) fn generate_vtables(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), String> {
        use crate::type_checker::context::class_needs_vtable;
        use cranelift_module::{FuncOrDataId, Linkage, Module};

        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        let call_conv = isa.default_call_conv();

        // Collect concrete classes that need vtables, sorted for deterministic output.
        let mut classes_needing_vtable: Vec<(
            &str,
            &crate::type_checker::context::ClassDefinition,
        )> = type_definitions
            .iter()
            .filter_map(|(name, def)| {
                if let TypeDefinition::Class(cd) = def {
                    // Concrete (not abstract), not generic, and has an abstract ancestor
                    if !cd.is_abstract
                        && cd.generics.is_none()
                        && class_needs_vtable(name, type_definitions)
                    {
                        return Some((name.as_str(), cd));
                    }
                }
                None
            })
            .collect();
        classes_needing_vtable.sort_unstable_by_key(|(name, _)| *name);

        for (class_name, _class_def) in &classes_needing_vtable {
            // Walk the full inheritance chain to collect ALL non-constructor methods
            // from ALL abstract ancestors. Methods from more-derived ancestors take
            // precedence (appear first), so we reverse to get base-first ordering.
            // A BTreeMap is used for deterministic (alphabetical) ordering within
            // each ancestor's contribution.
            //
            // For example, for `Base { greet } → Middle {} → Leaf`:
            //   abstract_chain = ["Middle", "Base"]
            //   methods from "Middle" = {} (none)
            //   methods from "Base" = {"greet" at slot 0}
            //   final vtable_methods = ["greet"]
            let mut abstract_chain: Vec<String> = Vec::new();
            let mut current = class_name.to_string();
            loop {
                match type_definitions.get(&current) {
                    Some(TypeDefinition::Class(cd)) if cd.is_abstract => {
                        abstract_chain.push(current.clone());
                        match cd.base_class.clone() {
                            Some(base) => current = base,
                            None => break,
                        }
                    }
                    Some(TypeDefinition::Class(cd)) => match cd.base_class.clone() {
                        Some(base) => current = base,
                        None => break,
                    },
                    _ => break,
                }
            }

            // Collect methods from all abstract ancestors. We iterate from the topmost
            // abstract ancestor downward so that when there are conflicting names, the
            // most-derived abstract class wins (though in practice abstract classes
            // don't redeclare each other's methods).
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let mut vtable_methods: Vec<String> = Vec::new();
            for ancestor in abstract_chain.iter().rev() {
                if let Some(TypeDefinition::Class(cd)) = type_definitions.get(ancestor) {
                    for (method_name, method_info) in &cd.methods {
                        if !method_info.is_constructor && !seen.contains(method_name) {
                            seen.insert(method_name.clone());
                            vtable_methods.push(method_name.clone());
                        }
                    }
                }
            }

            // Also collect methods from all traits implemented by this class or any
            // ancestor, appending those not already covered by the abstract chain.
            {
                use crate::type_checker::context::collect_trait_vtable_methods;
                let mut walk = class_name.to_string();
                while let Some(TypeDefinition::Class(cd)) = type_definitions.get(&walk) {
                    let base = cd.base_class.clone();
                    let traits = cd.traits.clone();
                    for trait_name in &traits {
                        let trait_methods =
                            collect_trait_vtable_methods(type_definitions, trait_name);
                        for m in trait_methods {
                            if !seen.contains(&m) {
                                seen.insert(m.clone());
                                vtable_methods.push(m);
                            }
                        }
                    }
                    match base {
                        Some(b) => walk = b,
                        None => break,
                    }
                }
            }

            vtable_methods.sort(); // deterministic alphabetical order

            if vtable_methods.is_empty() {
                continue;
            }

            let num_slots = vtable_methods.len();
            let vtable_size = (num_slots * ptr_size as usize) as u32;

            // Declare the vtable data as Export (may already be declared as Import by constructors).
            let vtable_sym = format!("__vtable_{}", class_name);
            let vtable_data_id = module
                .declare_data(&vtable_sym, Linkage::Export, false, false)
                .map_err(|e| format!("Failed to declare vtable {}: {}", vtable_sym, e))?;

            // Build the data description with function-pointer relocations.
            let mut desc = cranelift_module::DataDescription::new();
            desc.set_align(ptr_size as u64);
            // Initialize with zeros; relocations will fill in the actual addresses.
            desc.define(vec![0u8; vtable_size as usize].into_boxed_slice());

            for (slot_idx, method_name) in vtable_methods.iter().enumerate() {
                // Resolve the implementing function via inheritance.
                let func_name =
                    Self::resolve_vtable_method(class_name, method_name, type_definitions);
                let Some(func_name) = func_name else {
                    continue;
                };

                // Look up the already-compiled function in the module by name.
                let func_id = match module.get_name(&func_name) {
                    Some(FuncOrDataId::Func(id)) => id,
                    _ => {
                        // Function not compiled yet — declare as Import with a placeholder sig.
                        // This covers abstract base class concrete methods that may not be
                        // called directly from user code (only via vtable).
                        let mut sig = cranelift_codegen::ir::Signature::new(call_conv);
                        sig.params
                            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
                        module
                            .declare_function(&func_name, Linkage::Import, &sig)
                            .map_err(|e| {
                                format!("Failed to declare vtable fn {}: {}", func_name, e)
                            })?
                    }
                };

                let func_ref = module.declare_func_in_data(func_id, &mut desc);
                let slot_offset = (slot_idx * ptr_size as usize) as u32;
                desc.write_function_addr(slot_offset, func_ref);
            }

            module
                .define_data(vtable_data_id, &desc)
                .map_err(|e| format!("Failed to define vtable {}: {}", vtable_sym, e))?;
        }

        Ok(())
    }

    /// Resolve which function implements `method_name` for `class_name` via inheritance.
    /// Returns `"ClassName_methodName"` for the first class in the chain that defines it,
    /// or `"TraitName_methodName"` if the method is a default trait implementation.
    fn resolve_vtable_method(
        class_name: &str,
        method_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        let mut current = class_name.to_string();
        let mut all_traits: Vec<String> = Vec::new();
        while let Some(TypeDefinition::Class(cd)) = type_definitions.get(&current) {
            if let Some(method) = cd.methods.get(method_name) {
                // Only use this implementation if it has a body (not abstract).
                if !method.is_abstract {
                    return Some(format!("{}_{}", current, method_name));
                }
            }
            all_traits.extend(cd.traits.iter().cloned());
            match cd.base_class.clone() {
                Some(base) => current = base,
                None => break,
            }
        }
        // Fall back to a default trait method implementation.
        let mut visited = std::collections::HashSet::new();
        let mut trait_stack = all_traits;
        while let Some(t_name) = trait_stack.pop() {
            if !visited.insert(t_name.clone()) {
                continue;
            }
            if let Some(TypeDefinition::Trait(td)) = type_definitions.get(&t_name) {
                if let Some(method) = td.methods.get(method_name) {
                    if !method.is_abstract {
                        return Some(format!("{}_{}", t_name, method_name));
                    }
                }
                trait_stack.extend(td.parent_traits.iter().cloned());
            }
        }
        None
    }

    /// Returns true if a named Custom type has at least one managed field.
    ///
    /// Used to decide whether to call `__drop_TypeName` (when there are managed
    /// fields to clean up) or just `libc::free` (when all fields are primitives).
    pub(crate) fn has_managed_fields(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match type_defs.get(name) {
            Some(TypeDefinition::Struct(def)) => def
                .fields
                .iter()
                .any(|(_, ty, _)| is_field_managed(&ty.kind)),
            Some(TypeDefinition::Class(def)) => def
                .fields
                .iter()
                .any(|(_, fi)| is_field_managed(&fi.ty.kind)),
            Some(TypeDefinition::Enum(def)) => def
                .variants
                .values()
                .any(|fields| fields.iter().any(|ty| is_field_managed(&ty.kind))),
            _ => false,
        }
    }
}

/// Returns true if a field type is managed (heap-allocated, needs DecRef on drop).
fn is_field_managed(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Option(_)
            | TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Tuple(_)
            | TypeKind::Custom(_, _)
    )
}
