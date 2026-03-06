// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use crate::ast::expression::ExpressionKind;
use crate::ast::types::{Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{BasicBlock, Body, Local, Place, PlaceElem};
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
    pub(crate) rt_list_new_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_push_id: Option<cranelift_module::FuncId>,
    pub(crate) rt_list_free_id: Option<cranelift_module::FuncId>,
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

        // Translate each basic block
        for (idx, block_data) in body.basic_blocks.iter().enumerate() {
            let block = blocks[&BasicBlock(idx)];
            builder.switch_to_block(block);

            let mut module_ctx = ModuleCtx {
                module,
                string_literals,
                malloc_func_id: None,
                free_func_id: None,
                rt_array_new_id: None,
                rt_array_free_id: None,
                rt_list_new_id: None,
                rt_list_push_id: None,
                rt_list_free_id: None,
            };
            let type_ctx = TypeCtx {
                local_types: &self.local_types,
                type_definitions: self.type_definitions,
                ptr_type: self.ptr_type,
            };

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
                        builder, value, idx_val, base_type, type_ctx,
                    )?;
                }
            }
        }

        Ok(value)
    }
    /// Cast a value to instances of another type.
    pub(crate) fn cast_value(
        builder: &mut FunctionBuilder,
        value: Value,
        from_ty: cranelift_codegen::ir::Type,
        to_ty: cranelift_codegen::ir::Type,
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
            } else {
                Ok(builder.ins().sextend(to_ty, value))
            }
        } else if from_ty.is_float() && to_ty.is_int() {
            // Float to integer conversion (signed)
            Ok(builder.ins().fcvt_to_sint(to_ty, value))
        } else if from_ty.is_int() && to_ty.is_float() {
            // Integer to float conversion (signed)
            Ok(builder.ins().fcvt_from_sint(to_ty, value))
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
                            builder, addr, idx_val, base_type, type_ctx,
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
                        builder, addr, idx_val, value, base_type, type_ctx,
                    )?;
                }
            }
        }
        Ok(())
    }
    /// Helper to call `miri_rt_array_new(elem_count, elem_size) -> *mut MiriArray`.
    pub(crate) fn call_rt_array_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_count: Value,
        elem_size: Value,
    ) -> Result<Value, String> {
        let ptr_type = builder.func.dfg.value_type(elem_count);
        let func_id = match ctx.rt_array_new_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type), AbiParam::new(ptr_type)],
                    returns: vec![AbiParam::new(ptr_type)],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("miri_rt_array_new", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare miri_rt_array_new: {}", e))?;
                ctx.rt_array_new_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[elem_count, elem_size]);
        Ok(builder.inst_results(call)[0])
    }

    /// Helper to call `miri_rt_array_free(ptr)`.
    pub(crate) fn call_rt_array_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        let func_id = match ctx.rt_array_free_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("miri_rt_array_free", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare miri_rt_array_free: {}", e))?;
                ctx.rt_array_free_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
    }

    /// Helper to call `miri_rt_list_new(elem_size) -> *mut MiriList`.
    pub(crate) fn call_rt_list_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, String> {
        let ptr_type = builder.func.dfg.value_type(elem_size);
        let func_id = match ctx.rt_list_new_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![AbiParam::new(ptr_type)],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("miri_rt_list_new", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare miri_rt_list_new: {}", e))?;
                ctx.rt_list_new_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[elem_size]);
        Ok(builder.inst_results(call)[0])
    }

    /// Helper to call `miri_rt_list_push(ptr, val)`.
    /// The value is passed as a pointer-sized integer (covers all primitive types).
    pub(crate) fn call_rt_list_push(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        val: Value,
    ) -> Result<(), String> {
        let ptr_type = builder.func.dfg.value_type(list_ptr);
        let func_id = match ctx.rt_list_push_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type), AbiParam::new(ptr_type)],
                    returns: vec![],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("miri_rt_list_push", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare miri_rt_list_push: {}", e))?;
                ctx.rt_list_push_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[list_ptr, val]);
        Ok(())
    }

    /// Helper to call `miri_rt_list_free(ptr)`.
    pub(crate) fn call_rt_list_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        let func_id = match ctx.rt_list_free_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("miri_rt_list_free", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare miri_rt_list_free: {}", e))?;
                ctx.rt_list_free_id = Some(id);
                id
            }
        };
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
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
            TypeKind::Custom(name, args) if name == "Array" || name == "List" => {
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
            _ => &TypeKind::Int,
        }
    }

    /// Returns true if the given type is a List (dynamic collection).
    pub(crate) fn is_list_type(kind: &TypeKind) -> bool {
        matches!(kind, TypeKind::List(_))
            || matches!(kind, TypeKind::Custom(name, _) if name == "List")
    }

    /// Returns true if the given type is an Array or List collection.
    pub(crate) fn is_collection_type(kind: &TypeKind) -> bool {
        matches!(kind, TypeKind::Array(_, _) | TypeKind::List(_))
            || matches!(kind, TypeKind::Custom(name, _) if name == "Array" || name == "List")
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
        if Self::is_list_type(kind) {
            Self::call_rt_list_free(builder, ctx, ptr)
        } else if Self::is_collection_type(kind) {
            Self::call_rt_array_free(builder, ctx, ptr)
        } else if let TypeKind::Custom(name, _) = kind {
            // Drop specialization: release managed fields before freeing the struct.
            Self::emit_struct_drop(builder, ctx, name, ptr, type_ctx)?;
            Self::call_libc_free(builder, ctx, header_ptr)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// Emits DecRef calls for all managed fields of a struct or enum.
    ///
    /// For structs, iterates all fields and emits a DecRef sequence for each
    /// managed (heap-allocated) field. For enums, reads the discriminant and
    /// conditionally DecRefs the active variant's managed fields.
    fn emit_struct_drop(
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

        match def.clone() {
            TypeDefinition::Struct(struct_def) => {
                for (field_idx, (_name, field_ty, _vis)) in struct_def.fields.iter().enumerate() {
                    if is_field_managed(&field_ty.kind) {
                        let (offset, _cl_ty) = layout::field_layout(
                            &TypeKind::Custom(type_name.to_string(), None),
                            field_idx,
                            type_ctx.type_definitions,
                            ptr_type,
                        );
                        let field_ptr =
                            builder
                                .ins()
                                .load(ptr_type, MemFlags::new(), payload_ptr, offset);
                        Self::emit_decref_value(builder, ctx, &field_ty.kind, field_ptr, type_ctx)?;
                    }
                }
            }
            TypeDefinition::Enum(enum_def) => {
                // Read discriminant at offset 0
                let disc = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), payload_ptr, 0);
                let ptr_size = ptr_type.bytes() as i32;

                // For each variant that has managed fields, emit a conditional DecRef
                for (variant_idx, (_variant_name, fields)) in enum_def.variants.iter().enumerate() {
                    let has_managed = fields.iter().any(|ty| is_field_managed(&ty.kind));
                    if !has_managed {
                        continue;
                    }

                    // Check if this variant is active
                    let variant_val = builder.ins().iconst(ptr_type, variant_idx as i64);
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
                    for (field_idx, field_ty) in fields.iter().enumerate() {
                        if is_field_managed(&field_ty.kind) {
                            let field_offset = ptr_size + (field_idx as i32 * ptr_size);
                            let field_ptr = builder.ins().load(
                                ptr_type,
                                MemFlags::new(),
                                payload_ptr,
                                field_offset,
                            );
                            Self::emit_decref_value(
                                builder,
                                ctx,
                                &field_ty.kind,
                                field_ptr,
                                type_ctx,
                            )?;
                        }
                    }
                    builder.ins().jump(merge_block, &[]);
                    builder.seal_block(drop_block);
                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                }
            }
            TypeDefinition::Class(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Generic(_) => {}
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

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);

        // Skip immortal objects (RC < 0)
        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        let merge_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        builder.switch_to_block(dec_block);
        let new_rc = builder.ins().iadd_imm(rc, -1);
        builder.ins().store(MemFlags::new(), new_rc, header_ptr, 0);

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

        builder.seal_block(dec_block);
        builder.seal_block(free_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

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
        base_value: Value,
        idx_val: Value,
        base_type: &Type,
        type_ctx: &TypeCtx,
    ) -> Result<Value, String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

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

        // Runtime bounds check: trap if index >= length
        let oob = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx_val,
            len_val,
        );
        builder.ins().trapnz(oob, TrapCode::unwrap_user(1));

        // Compute element address: data + index * elem_size
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);

        // Load the element
        Ok(builder
            .ins()
            .load(cl_elem_ty, MemFlags::new(), elem_addr, 0))
    }

    /// Translates an index write on a collection (Array or List).
    /// Same layout assumptions as `translate_collection_index_read`.
    pub(crate) fn translate_collection_index_write(
        builder: &mut FunctionBuilder,
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
        builder.ins().trapnz(oob, TrapCode::unwrap_user(1));

        // Compute element address and store
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);
        builder.ins().store(MemFlags::new(), value, elem_addr, 0);
        Ok(())
    }

    /// Helper to call libc malloc, caching the FuncId across invocations.
    pub(crate) fn call_libc_malloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        size: Value,
    ) -> Result<Value, String> {
        let ptr_type = builder.func.dfg.value_type(size);
        let func_id = match ctx.malloc_func_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![AbiParam::new(ptr_type)],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("malloc", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare malloc: {}", e))?;
                ctx.malloc_func_id = Some(id);
                id
            }
        };

        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[size]);
        Ok(builder.inst_results(call)[0])
    }
    /// Helper to call libc free, caching the FuncId across invocations.
    pub(crate) fn call_libc_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), String> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        let func_id = match ctx.free_func_id {
            Some(id) => id,
            None => {
                let sig = Signature {
                    params: vec![AbiParam::new(ptr_type)],
                    returns: vec![],
                    call_conv: builder.func.signature.call_conv,
                };
                let id = ctx
                    .module
                    .declare_function("free", Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare free: {}", e))?;
                ctx.free_func_id = Some(id);
                id
            }
        };

        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
    }
}

/// Returns true if a field type is managed (heap-allocated, needs DecRef on drop).
fn is_field_managed(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Custom(_, _)
    )
}
