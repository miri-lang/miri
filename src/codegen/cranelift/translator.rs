// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::types::translate_type;
use crate::error::CodegenError;
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
///
/// Runtime-symbol `FuncId`s are cached in a single map keyed by symbol name.
/// `call_cached_func` declares an import the first time a symbol is touched
/// and returns the cached id on subsequent calls.
pub(crate) struct ModuleCtx<'a> {
    pub(crate) module: &'a mut ObjectModule,
    pub(crate) string_literals: &'a mut HashMap<String, String>,
    pub(crate) cached_funcs: HashMap<&'static str, cranelift_module::FuncId>,
    pub(crate) kernel_registry:
        &'a HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
}

/// Context for type information during translation.
pub(crate) struct TypeCtx<'a> {
    pub(crate) local_types: &'a [&'a Type],
    pub(crate) type_definitions: &'a HashMap<String, TypeDefinition>,
    pub(crate) ptr_type: cl_types::Type,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Used by `read_place` and `resolve_projected_type_kind` to translate
    /// `Field(i)` projections on closure locals into the correct capture type.
    pub(crate) closure_capture_ast_types: &'a HashMap<crate::mir::Local, Vec<Type>>,
    /// For scalar `out` parameters: maps each param Local to the Cranelift Variable
    /// that holds the incoming pointer. Used by the Return terminator to write back.
    pub(crate) out_param_ptr_vars: &'a HashMap<Local, Variable>,
}

/// One Cranelift runtime call site: which symbol to declare-and-call, its
/// parameter / return types, and the argument values to pass.
pub(crate) struct CallSite<'a> {
    pub(crate) name: &'static str,
    pub(crate) param_types: &'a [cranelift_codegen::ir::Type],
    pub(crate) return_types: &'a [cranelift_codegen::ir::Type],
    pub(crate) args: &'a [Value],
}

/// Build an empty `ModuleCtx` with an unpopulated FuncId cache.
/// Caches populate lazily as code generation needs each runtime symbol.
pub(crate) fn empty_module_ctx<'a>(
    module: &'a mut ObjectModule,
    string_literals: &'a mut HashMap<String, String>,
    kernel_registry: &'a HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
) -> ModuleCtx<'a> {
    ModuleCtx {
        module,
        string_literals,
        cached_funcs: HashMap::new(),
        kernel_registry,
    }
}

/// Normalized element type for runtime decref/clone helper dispatch.
///
/// Collapses canonical `TypeKind::{List,Array,Set,Map}` and the post-
/// normalization `TypeKind::Custom(name, _)` form (where
/// `BuiltinCollectionKind::from_name(name)` is `Some`) into a single
/// `Builtin(BuiltinCollectionKind)` variant so dispatch sites match once
/// per shape instead of duplicating arms for each spelling.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ElementShape<'a> {
    /// String element — uses `miri_rt_string_decref_element`.
    String,
    /// Built-in collection element (List/Array/Set/Map) — uses the matching
    /// `miri_rt_{kind}_decref_element` helper.
    Builtin(BuiltinCollectionKind),
    /// User-defined class — uses the `__decref_TypeName` thunk; clone via
    /// `__clone_TypeName` only when the class implements `Cloneable`.
    UserClass(&'a str),
    /// Anything else (primitives, void, errors) — no decref needed.
    Other,
}

/// Returns true when a type requires pointer-passing for `out` semantics.
///
/// Managed heap types (List, String, custom classes, etc.) are already pointers;
/// their mutations are visible to callers without extra indirection.
/// Primitive scalars (int, bool, floats) are passed by value and need a pointer
/// so that callee modifications propagate back.
pub(crate) fn needs_out_pointer(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::U8
            | TypeKind::I16
            | TypeKind::U16
            | TypeKind::I32
            | TypeKind::U32
            | TypeKind::I64
            | TypeKind::U64
            | TypeKind::I128
            | TypeKind::U128
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Float
            | TypeKind::Boolean
    )
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
        kernel_registry: &HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
    ) -> Result<(), CodegenError> {
        self.build_signature(body)?;
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        let (locals, blocks) = Self::declare_locals_and_blocks(&mut builder, body, self.ptr_type);
        let out_param_ptr_vars =
            Self::setup_entry_params(&mut builder, body, &locals, &blocks, self.ptr_type);

        let mut module_ctx = empty_module_ctx(module, string_literals, kernel_registry);
        let type_ctx = TypeCtx {
            local_types: &self.local_types,
            type_definitions: self.type_definitions,
            ptr_type: self.ptr_type,
            closure_capture_ast_types: &body.closure_capture_types,
            out_param_ptr_vars: &out_param_ptr_vars,
        };

        Self::translate_blocks(
            &mut builder,
            &mut module_ctx,
            body,
            &locals,
            &blocks,
            &out_param_ptr_vars,
            &type_ctx,
            self.ptr_type,
        )?;

        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Declare a Cranelift variable per MIR local and a Cranelift block per
    /// MIR basic block. The entry block (idx 0) is wired up with function
    /// parameters.
    fn declare_locals_and_blocks(
        builder: &mut FunctionBuilder,
        body: &Body,
        ptr_type: cl_types::Type,
    ) -> (HashMap<Local, Variable>, HashMap<BasicBlock, Block>) {
        let mut locals: HashMap<Local, Variable> = HashMap::with_capacity(body.local_decls.len());
        let mut blocks: HashMap<BasicBlock, Block> =
            HashMap::with_capacity(body.basic_blocks.len());
        for (idx, local_decl) in body.local_decls.iter().enumerate() {
            let cl_type = translate_type(&local_decl.ty, ptr_type);
            let var = builder.declare_var(cl_type);
            locals.insert(Local(idx), var);
        }
        for idx in 0..body.basic_blocks.len() {
            let cl_block = builder.create_block();
            blocks.insert(BasicBlock(idx), cl_block);
            if idx == 0 {
                builder.append_block_params_for_function_params(cl_block);
            }
        }
        (locals, blocks)
    }

    /// Bind entry-block parameters to their locals. Scalar `out` params arrive
    /// as pointers — the pointer is saved into a `Variable` for later writeback
    /// at Return; the value load is deferred to the main loop so the entry
    /// block stays Pristine until `switch_to_block` runs.
    fn setup_entry_params(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        ptr_type: cl_types::Type,
    ) -> HashMap<Local, Variable> {
        let mut out_param_ptr_vars: HashMap<Local, Variable> = HashMap::new();
        let Some(&entry_block) = blocks.get(&BasicBlock(0)) else {
            return out_param_ptr_vars;
        };
        builder.switch_to_block(entry_block);
        let params_vec: Vec<Value> = builder.block_params(entry_block).to_vec();
        for (i, param_val) in params_vec.into_iter().enumerate() {
            let local = Local(i + 1);
            let is_scalar_out = i < body.out_params.len()
                && body.out_params[i]
                && body
                    .local_decls
                    .get(local.0)
                    .is_some_and(|d| needs_out_pointer(&d.ty.kind));
            if is_scalar_out {
                let ptr_var = builder.declare_var(ptr_type);
                builder.def_var(ptr_var, param_val);
                out_param_ptr_vars.insert(local, ptr_var);
            } else if let Some(&var) = locals.get(&local) {
                builder.def_var(var, param_val);
            }
        }
        out_param_ptr_vars
    }

    /// Lower MIR statements + terminator for every basic block. Block 0 also
    /// runs the deferred scalar-out-param load and closure-capture extraction
    /// (must happen after `switch_to_block` per Cranelift's block-state rules).
    #[allow(clippy::too_many_arguments)]
    fn translate_blocks(
        builder: &mut FunctionBuilder,
        module_ctx: &mut ModuleCtx,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        out_param_ptr_vars: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        for (idx, block_data) in body.basic_blocks.iter().enumerate() {
            let block = blocks[&BasicBlock(idx)];
            builder.switch_to_block(block);

            if idx == 0 {
                Self::load_scalar_out_params(builder, body, locals, out_param_ptr_vars, ptr_type);
                if !body.env_capture_locals.is_empty() {
                    Self::load_closure_captures(builder, body, locals, ptr_type);
                }
            }

            for stmt in &block_data.statements {
                Self::translate_statement(builder, module_ctx, stmt, locals, type_ctx)?;
            }
            if let Some(ref terminator) = block_data.terminator {
                Self::translate_terminator(
                    builder, module_ctx, terminator, body, locals, blocks, type_ctx,
                )?;
            }
        }
        Ok(())
    }

    /// Load initial scalar `out` param values from their caller-provided
    /// pointers into the matching `Local` Variables.
    fn load_scalar_out_params(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        out_param_ptr_vars: &HashMap<Local, Variable>,
        ptr_type: cl_types::Type,
    ) {
        for (&out_local, &ptr_var) in out_param_ptr_vars {
            let ptr = builder.use_var(ptr_var);
            let val_cl_type = translate_type(&body.local_decls[out_local.0].ty, ptr_type);
            let val = builder.ins().load(val_cl_type, MemFlags::new(), ptr, 0);
            if let Some(&var) = locals.get(&out_local) {
                builder.def_var(var, val);
            }
        }
    }

    /// Closure bodies: load captured values from `env_ptr` (Local 1) at the
    /// top of the entry block. Closure layout: `payload[0]=fn_ptr,
    /// payload[1]=dtor_ptr, payload[2+i]=cap_i`. Loads as ptr_type, then
    /// reduces to the capture's target Cranelift type if narrower.
    fn load_closure_captures(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        ptr_type: cl_types::Type,
    ) {
        let env_ptr_var = locals[&Local(1)];
        let env_ptr_val = builder.use_var(env_ptr_var);
        for (i, &cap_local) in body.env_capture_locals.iter().enumerate() {
            let offset = (i + 2) as i64 * ptr_type.bytes() as i64;
            let cap_ptr = builder.ins().iadd_imm(env_ptr_val, offset);
            let cap_cl_type = translate_type(&body.local_decls[cap_local.0].ty, ptr_type);
            let raw_val = builder.ins().load(ptr_type, MemFlags::new(), cap_ptr, 0);
            let cap_val = if cap_cl_type == ptr_type {
                raw_val
            } else if cap_cl_type.is_int() && cap_cl_type.bits() < ptr_type.bits() {
                builder.ins().ireduce(cap_cl_type, raw_val)
            } else {
                raw_val
            };
            if let Some(&cap_var) = locals.get(&cap_local) {
                builder.def_var(cap_var, cap_val);
            }
        }
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
    pub(crate) fn build_signature(&mut self, body: &Body) -> Result<(), CodegenError> {
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
                // Scalar out params are passed as pointers (copy-in/copy-out ABI).
                let cl_type = if body.out_params.get(i - 1).copied().unwrap_or(false)
                    && needs_out_pointer(&param_ty.kind)
                {
                    self.ptr_type
                } else {
                    translate_type(param_ty, self.ptr_type)
                };
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
    ) -> Result<Value, CodegenError> {
        let local_types = type_ctx.local_types;
        let type_definitions = type_ctx.type_definitions;
        let ptr_type = type_ctx.ptr_type;
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;

        let mut value = builder.use_var(*var);

        for proj in &place.projection {
            match proj {
                PlaceElem::Deref => {
                    value = builder.ins().load(ptr_type, MemFlags::new(), value, 0);
                }
                PlaceElem::Field(idx) => {
                    let base_type = &local_types[place.local.0];
                    if matches!(base_type.kind, TypeKind::Function(_)) {
                        // Closure env field: capture `idx` lives at
                        // payload_ptr + (idx+2)*ptr_size (slot 0=fn_ptr, slot 1=dtor_ptr).
                        let offset = (*idx as i64 + 2) * ptr_type.bytes() as i64;
                        value = builder
                            .ins()
                            .load(ptr_type, MemFlags::new(), value, offset as i32);
                    } else {
                        let (offset, field_ty) =
                            layout::field_layout(&base_type.kind, *idx, type_definitions, ptr_type);
                        value = builder.ins().load(field_ty, MemFlags::new(), value, offset);
                    }
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals.get(local).ok_or_else(|| {
                        CodegenError::Internal(format!("Unknown index local: {:?}", local))
                    })?;
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
    ) -> Result<Value, CodegenError> {
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
            Err(CodegenError::Internal(format!(
                "Unsupported implicit cast from {} to {}",
                from_ty, to_ty
            )))
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
    ) -> Result<(), CodegenError> {
        if place.projection.is_empty() {
            let var = locals.get(&place.local).ok_or_else(|| {
                CodegenError::Internal(format!("Unknown local: {:?}", place.local))
            })?;
            builder.def_var(*var, value);
            return Ok(());
        }
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let base_addr = builder.use_var(*var);
        let (addr, current_type) =
            Self::walk_projection_path(builder, ctx, place, base_addr, locals, type_ctx)?;
        Self::store_at_last_projection(
            builder,
            ctx,
            place,
            value,
            addr,
            current_type,
            locals,
            type_ctx,
        )
    }

    /// Walk every projection on `place` *except the last*, advancing `addr`
    /// and tracking the type at the current depth. Returns the address that
    /// the final projection should consume and the type of the value at that
    /// address.
    fn walk_projection_path<'tc>(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        base_addr: Value,
        locals: &HashMap<Local, Variable>,
        type_ctx: &'tc TypeCtx,
    ) -> Result<(Value, &'tc Type), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let type_definitions = type_ctx.type_definitions;
        let mut addr = base_addr;
        let mut current_type: &Type = type_ctx.local_types[place.local.0];

        for proj in &place.projection[..place.projection.len() - 1] {
            match proj {
                PlaceElem::Deref => {
                    addr = builder.ins().load(ptr_type, MemFlags::new(), addr, 0);
                }
                PlaceElem::Field(idx) => {
                    let (offset, _) =
                        layout::field_layout(&current_type.kind, *idx, type_definitions, ptr_type);
                    addr = builder.ins().iadd_imm(addr, offset as i64);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals.get(local).ok_or_else(|| {
                        CodegenError::Internal(format!("Unknown index local: {:?}", local))
                    })?;
                    let idx_val = builder.use_var(*idx_var);
                    addr = Self::translate_collection_index_read(
                        builder,
                        ctx,
                        addr,
                        idx_val,
                        current_type,
                        type_ctx,
                    )?;
                    if let Some(elem_type) =
                        Self::resolve_collection_elem_type_as_type(current_type)
                    {
                        current_type = elem_type;
                    }
                }
            }
        }
        Ok((addr, current_type))
    }

    /// Apply the final projection on `place` as a store of `value`.
    /// `addr` and `current_type` come from `walk_projection_path`.
    #[allow(clippy::too_many_arguments)]
    fn store_at_last_projection(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        value: Value,
        addr: Value,
        current_type: &Type,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let last_proj = place.projection.last().ok_or_else(|| {
            CodegenError::Internal(
                "assign_to_place: empty projection after non-empty check".to_string(),
            )
        })?;
        match last_proj {
            PlaceElem::Deref => {
                builder.ins().store(MemFlags::new(), value, addr, 0);
            }
            PlaceElem::Field(idx) => {
                let (offset, _) = layout::field_layout(
                    &current_type.kind,
                    *idx,
                    type_ctx.type_definitions,
                    type_ctx.ptr_type,
                );
                builder.ins().store(MemFlags::new(), value, addr, offset);
            }
            PlaceElem::Index(local) => {
                let idx_var = locals.get(local).ok_or_else(|| {
                    CodegenError::Internal(format!("Unknown index local: {:?}", local))
                })?;
                let idx_val = builder.use_var(*idx_var);
                Self::translate_collection_index_write(
                    builder,
                    ctx,
                    addr,
                    idx_val,
                    value,
                    current_type,
                    type_ctx,
                )?;
            }
        }
        Ok(())
    }
    /// Look up a cached `FuncId` for `name`, or declare it as a runtime import
    /// when missing. Used by helpers that need the `FuncId` itself (not just an
    /// instruction emit) — e.g. taking the address of a runtime function for
    /// passing as a callback.
    fn get_or_declare_runtime_func(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
        param_types: &[cranelift_codegen::ir::Type],
        return_types: &[cranelift_codegen::ir::Type],
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        if let Some(&id) = ctx.cached_funcs.get(name) {
            return Ok(id);
        }
        let sig = Signature {
            params: param_types.iter().map(|&t| AbiParam::new(t)).collect(),
            returns: return_types.iter().map(|&t| AbiParam::new(t)).collect(),
            call_conv: builder.func.signature.call_conv,
        };
        let id = ctx
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;
        ctx.cached_funcs.insert(name, id);
        Ok(id)
    }

    /// Declare-and-cache a runtime function, then call it.
    ///
    /// `module` and `cached_funcs` are passed separately to avoid double-
    /// borrowing `ModuleCtx`. The cache is keyed by the symbol name and
    /// populates lazily on first use.
    pub(crate) fn call_cached_func(
        builder: &mut FunctionBuilder,
        module: &mut ObjectModule,
        cached_funcs: &mut HashMap<&'static str, cranelift_module::FuncId>,
        call: CallSite,
    ) -> Result<cranelift_codegen::ir::Inst, CodegenError> {
        let CallSite {
            name,
            param_types,
            return_types,
            args,
        } = call;
        let func_id = if let Some(&id) = cached_funcs.get(name) {
            id
        } else {
            let sig = Signature {
                params: param_types.iter().map(|&t| AbiParam::new(t)).collect(),
                returns: return_types.iter().map(|&t| AbiParam::new(t)).collect(),
                call_conv: builder.func.signature.call_conv,
            };
            let id = module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;
            cached_funcs.insert(name, id);
            id
        };
        let local_func = module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().call(local_func, args))
    }

    pub(crate) fn call_rt_array_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_count: Value,
        elem_size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(elem_count);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_NEW,
                param_types: &[pt, pt],
                return_types: &[pt],
                args: &[elem_count, elem_size],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_array_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_list_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(elem_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_NEW,
                param_types: &[pt],
                return_types: &[pt],
                args: &[elem_size],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_list_push(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        val: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_PUSH,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, val],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_list_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_map_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_size: Value,
        value_size: Value,
        key_kind: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(key_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_NEW,
                param_types: &[pt, pt, pt],
                return_types: &[pt],
                args: &[key_size, value_size, key_kind],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_map_set(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        key: Value,
        value: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET,
                param_types: &[pt, pt, pt],
                return_types: &[],
                args: &[map_ptr, key, value],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_map_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of `miri_rt_list_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_list_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::LIST_DECREF_ELEMENT, ptr_type)
    }

    /// Look up (or declare) a `miri_rt_*_decref_element(ptr)` runtime helper
    /// and return its function address as a ptr-sized integer.
    fn decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let func_id = Self::get_or_declare_runtime_func(builder, ctx, name, &[ptr_type], &[])?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_map_set_val_drop_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_val_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_VAL_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_key_drop_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_key_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_KEY_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of `miri_rt_string_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_string_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::STRING_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_array_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_array_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::ARRAY_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_set_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_set_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::SET_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_map_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_map_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::MAP_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `__decref_{type_name}` as a ptr-sized integer.
    ///
    /// Used as `elem_drop_fn` for List/Set/Map holding custom-type elements.
    /// The function is generated by `generate_drop_function` with Export linkage
    /// so declaring it here as Import allows the linker to resolve it.
    pub(crate) fn get_custom_decref_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let mut decref_name = String::with_capacity(9 + type_name.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(type_name);
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&decref_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_list_set_elem_drop_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_array_set_elem_drop_fn(array_ptr, fn_ptr)`.
    pub(crate) fn call_rt_array_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(array_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[array_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of `__clone_{type_name}` as a ptr-sized integer.
    ///
    /// Used as `elem_clone_fn` for Array/List/Set holding custom-type elements.
    /// The function is generated by `generate_clone_function` with Export linkage.
    pub(crate) fn get_custom_clone_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let mut clone_name = String::with_capacity(9 + type_name.len());
        clone_name.push_str("__clone_");
        clone_name.push_str(type_name);
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![AbiParam::new(ptr_type)],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&clone_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(clone_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_array_set_elem_clone_fn(array_ptr, fn_ptr)`.
    pub(crate) fn call_rt_array_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(array_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[array_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_list_set_elem_clone_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_set_set_elem_clone_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[set_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_val_clone_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_val_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_VAL_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_set_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(elem_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_NEW,
                param_types: &[pt],
                return_types: &[pt],
                args: &[elem_size],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_set_add(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        elem: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_ADD,
                param_types: &[pt, pt],
                return_types: &[cl_types::I8],
                args: &[set_ptr, elem],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_set_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_set_set_elem_drop_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[set_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_string_free(ptr)`.
    ///
    /// `ptr` must be the payload pointer (past the RC header) returned by a
    /// `miri_rt_string_*` constructor.  The runtime function runs the
    /// `MiriString` destructor (freeing the inner data buffer) and then frees
    /// the `[RC][payload]` block via `free_with_rc`.
    pub(crate) fn call_rt_string_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::STRING_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_closure_alloc_track()` to record a closure malloc in
    /// `CLOSURE_ALLOC_BALANCE`. Must be matched by `call_rt_closure_free_track`.
    pub(crate) fn call_rt_closure_alloc_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<(), CodegenError> {
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLOSURE_ALLOC_TRACK,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_closure_free_track()` to record a closure free in
    /// `CLOSURE_ALLOC_BALANCE`. Must follow a matching `call_rt_closure_alloc_track`.
    pub(crate) fn call_rt_closure_free_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<(), CodegenError> {
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLOSURE_FREE_TRACK,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
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

    pub(crate) fn call_rt_array_panic_oob(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        index: Value,
        len: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(index);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_PANIC_OOB,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[index, len],
            },
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
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        // No element type ⇒ assume pointer-sized addressing (managed collections
        // hold pointers; this matches the historical silent fallback but is now
        // explicit at the call site rather than inside the resolver).
        let cl_elem_ty = match Self::resolve_collection_elem_type(base_type) {
            Some(elem_kind) => {
                crate::codegen::cranelift::types::translate_type_kind(elem_kind, ptr_type)
            }
            None => ptr_type,
        };
        let elem_size = cl_elem_ty.bytes() as i64;

        // Tuples store `[count][field0][field1]...` at `base_value`; the
        // `data` field is implicit (fields start right after the count slot).
        // Also covers the post-normalization `Custom(TUPLE_TYPE_NAME, ...)`
        // form produced for homogeneous tuples by the type checker.
        let is_tuple_type = base_type.kind.is_tuple();

        let (len_val, fields_base) = if is_tuple_type {
            let len = builder.ins().load(ptr_type, MemFlags::new(), base_value, 0);
            let base = builder.ins().iadd_imm(base_value, ptr_size as i64);
            (len, base)
        } else {
            // MiriArray/MiriList layout: { data: *mut u8, len: usize, ... }
            let data = builder.ins().load(ptr_type, MemFlags::new(), base_value, 0);
            let len = builder
                .ins()
                .load(ptr_type, MemFlags::new(), base_value, ptr_size);
            (len, data)
        };
        let elem_addr = Self::bounds_checked_elem_addr(
            builder,
            ctx,
            fields_base,
            idx_val,
            len_val,
            elem_size,
            ptr_type,
        )?;
        Ok(builder
            .ins()
            .load(cl_elem_ty, MemFlags::new(), elem_addr, 0))
    }

    /// Emit a runtime bounds-check (`idx_val < len_val`) that traps on failure
    /// and otherwise computes `fields_base + idx_val * elem_size`. Both the
    /// panic and continuation blocks are sealed before returning.
    fn bounds_checked_elem_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        fields_base: Value,
        idx_val: Value,
        len_val: Value,
        elem_size: i64,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
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
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(fields_base, byte_offset);

        builder.seal_block(panic_block);
        builder.seal_block(cont_block);
        Ok(elem_addr)
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
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        // No element type ⇒ assume pointer-sized addressing (managed collections
        // hold pointers; this matches the historical silent fallback but is now
        // explicit at the call site rather than inside the resolver).
        let elem_type_kind = Self::resolve_collection_elem_type(base_type);
        let cl_elem_ty = match elem_type_kind {
            Some(elem_kind) => {
                crate::codegen::cranelift::types::translate_type_kind(elem_kind, ptr_type)
            }
            None => ptr_type,
        };
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

        // Compute element address
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);
        // DecRef the old managed element before overwriting. When the element
        // type is unknown (None), skip — the historical fallback treated it as
        // a primitive and emitted no decref.
        if let Some(elem_kind) = elem_type_kind {
            Self::emit_managed_elem_decref(builder, ctx, elem_addr, elem_kind, ptr_type, type_ctx)?;
        }
        builder.ins().store(MemFlags::new(), value, elem_addr, 0);

        builder.seal_block(panic_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Helper to call libc malloc, caching the FuncId across invocations.
    pub(crate) fn call_libc_malloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: "malloc",
                param_types: &[pt],
                return_types: &[pt],
                args: &[size],
            },
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
    ) -> Result<(), CodegenError> {
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
            &mut ctx.cached_funcs,
            CallSite {
                name: "free",
                param_types: &[ptr_type],
                return_types: &[],
                args: &[real_ptr],
            },
        )?;
        Ok(())
    }
}

/// Returns true if a field type is managed (heap-allocated, needs DecRef on drop).
pub(crate) fn is_field_managed(kind: &TypeKind) -> bool {
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

/// Returns true if a closure capture type is managed and needs DecRef when the closure drops.
/// Like `is_field_managed` but also includes `Function` (closures can capture other closures).
pub(crate) fn is_capture_managed(kind: &TypeKind) -> bool {
    matches!(kind, TypeKind::Function(_)) || is_field_managed(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expression::{Expression, ExpressionKind};
    use crate::ast::types::Type;
    use crate::error::syntax::Span;

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, Span::default())
    }

    fn expr_ty(kind: TypeKind) -> Expression {
        Expression {
            id: 0,
            node: ExpressionKind::Type(Box::new(ty(kind)), false),
            span: Span::default(),
        }
    }

    #[test]
    fn needs_out_pointer_yes_for_scalars() {
        for k in [
            TypeKind::Int,
            TypeKind::I8,
            TypeKind::U8,
            TypeKind::I16,
            TypeKind::U16,
            TypeKind::I32,
            TypeKind::U32,
            TypeKind::I64,
            TypeKind::U64,
            TypeKind::I128,
            TypeKind::U128,
            TypeKind::F32,
            TypeKind::F64,
            TypeKind::Float,
            TypeKind::Boolean,
        ] {
            assert!(needs_out_pointer(&k), "{:?} should need an out pointer", k);
        }
    }

    #[test]
    fn needs_out_pointer_no_for_managed() {
        assert!(!needs_out_pointer(&TypeKind::String));
        assert!(!needs_out_pointer(&TypeKind::List(Box::new(expr_ty(
            TypeKind::Int
        )))));
        assert!(!needs_out_pointer(&TypeKind::Custom(
            "MyClass".to_string(),
            None
        )));
        assert!(!needs_out_pointer(&TypeKind::Void));
    }

    #[test]
    fn classify_element_shape_built_in_canonical() {
        let int_expr = Box::new(expr_ty(TypeKind::Int));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::String),
            ElementShape::String
        ));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::List(int_expr.clone())),
            ElementShape::Builtin(BuiltinCollectionKind::List)
        ));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::Array(
                int_expr.clone(),
                Box::new(expr_ty(TypeKind::Int))
            )),
            ElementShape::Builtin(BuiltinCollectionKind::Array)
        ));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::Set(int_expr.clone())),
            ElementShape::Builtin(BuiltinCollectionKind::Set)
        ));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&TypeKind::Map(
                int_expr.clone(),
                int_expr.clone()
            )),
            ElementShape::Builtin(BuiltinCollectionKind::Map)
        ));
    }

    #[test]
    fn classify_element_shape_custom_collapses_to_builtin() {
        let name = BuiltinCollectionKind::List.name().to_string();
        let list_custom = TypeKind::Custom(name, Some(vec![expr_ty(TypeKind::Int)]));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&list_custom),
            ElementShape::Builtin(BuiltinCollectionKind::List)
        ));
    }

    #[test]
    fn classify_element_shape_custom_user_class() {
        let user = TypeKind::Custom("MyClass".to_string(), None);
        let shape = FunctionTranslator::classify_element_shape(&user);
        assert!(
            matches!(shape, ElementShape::UserClass("MyClass")),
            "expected UserClass(\"MyClass\"), got {:?}",
            shape
        );
    }

    #[test]
    fn classify_element_shape_primitives_are_other() {
        for k in [
            TypeKind::Int,
            TypeKind::Boolean,
            TypeKind::F32,
            TypeKind::Void,
        ] {
            assert!(matches!(
                FunctionTranslator::classify_element_shape(&k),
                ElementShape::Other
            ));
        }
    }

    #[test]
    fn is_field_managed_classifies_heap_types() {
        let int_expr = Box::new(expr_ty(TypeKind::Int));
        assert!(is_field_managed(&TypeKind::String));
        assert!(is_field_managed(&TypeKind::List(int_expr.clone())));
        assert!(is_field_managed(&TypeKind::Custom(
            "MyClass".to_string(),
            None
        )));
        assert!(!is_field_managed(&TypeKind::Int));
        assert!(!is_field_managed(&TypeKind::Boolean));
    }

    #[test]
    fn is_capture_managed_includes_functions() {
        use crate::ast::types::FunctionTypeData;
        let fn_kind = TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: Vec::new(),
            return_type: None,
        }));
        assert!(is_capture_managed(&fn_kind));
        // Reuses is_field_managed for everything else.
        assert!(is_capture_managed(&TypeKind::String));
        assert!(!is_capture_managed(&TypeKind::Int));
    }

    #[test]
    fn is_unsigned_type_kind_only_unsigned_integers() {
        for k in [
            TypeKind::U8,
            TypeKind::U16,
            TypeKind::U32,
            TypeKind::U64,
            TypeKind::U128,
        ] {
            assert!(FunctionTranslator::is_unsigned_type_kind(&k));
        }
        for k in [
            TypeKind::I8,
            TypeKind::Int,
            TypeKind::F32,
            TypeKind::Boolean,
        ] {
            assert!(!FunctionTranslator::is_unsigned_type_kind(&k));
        }
    }

    #[test]
    fn is_list_set_map_collection_type_predicates() {
        let int_expr = Box::new(expr_ty(TypeKind::Int));
        assert!(FunctionTranslator::is_list_type(&TypeKind::List(
            int_expr.clone()
        )));
        assert!(!FunctionTranslator::is_list_type(&TypeKind::Set(
            int_expr.clone()
        )));
        assert!(FunctionTranslator::is_set_type(&TypeKind::Set(
            int_expr.clone()
        )));
        assert!(FunctionTranslator::is_map_type(&TypeKind::Map(
            int_expr.clone(),
            int_expr.clone()
        )));
        assert!(FunctionTranslator::is_collection_type(&TypeKind::List(
            int_expr.clone()
        )));
        assert!(!FunctionTranslator::is_collection_type(&TypeKind::String));
    }
}
