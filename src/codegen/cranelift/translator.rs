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
) -> ModuleCtx<'a> {
    ModuleCtx {
        module,
        string_literals,
        cached_funcs: HashMap::new(),
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
    ) -> Result<(), CodegenError> {
        self.build_signature(body)?;
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        let (locals, blocks) = Self::declare_locals_and_blocks(&mut builder, body, self.ptr_type);
        let out_param_ptr_vars =
            Self::setup_entry_params(&mut builder, body, &locals, &blocks, self.ptr_type);

        let mut module_ctx = empty_module_ctx(module, string_literals);
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
    fn call_cached_func(
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

    /// Classify an element `TypeKind` into the shape used to pick the matching
    /// runtime decref/clone helper. Folds canonical built-in collection kinds
    /// (`TypeKind::List`, `TypeKind::Array`, ...) and the post-normalization
    /// `TypeKind::Custom` form (where `BuiltinCollectionKind::from_name` is
    /// `Some`) into a single `ElementShape::Builtin` representation so
    /// dispatch sites match once.
    pub(crate) fn classify_element_shape(kind: &TypeKind) -> ElementShape<'_> {
        match kind {
            TypeKind::String => ElementShape::String,
            TypeKind::List(_) => ElementShape::Builtin(BuiltinCollectionKind::List),
            TypeKind::Array(_, _) => ElementShape::Builtin(BuiltinCollectionKind::Array),
            TypeKind::Set(_) => ElementShape::Builtin(BuiltinCollectionKind::Set),
            TypeKind::Map(_, _) => ElementShape::Builtin(BuiltinCollectionKind::Map),
            TypeKind::Custom(name, _) => match BuiltinCollectionKind::from_name(name) {
                Some(builtin) => ElementShape::Builtin(builtin),
                None => ElementShape::UserClass(name),
            },
            _ => ElementShape::Other,
        }
    }

    /// Address of the runtime decref helper for an element of `shape`, or
    /// `None` when the shape needs no decref (primitives, void, etc.).
    pub(crate) fn elem_decref_addr_for_shape(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        shape: ElementShape,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let addr = match shape {
            ElementShape::String => {
                Self::get_rt_string_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::List) => {
                Self::get_rt_list_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Array) => {
                Self::get_rt_array_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Set) => {
                Self::get_rt_set_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Map) => {
                Self::get_rt_map_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::UserClass(name) => {
                Self::get_custom_decref_thunk_addr(builder, ctx, name, ptr_type)?
            }
            ElementShape::Other => return Ok(None),
        };
        Ok(Some(addr))
    }

    /// Address of the runtime clone helper for an element of `shape`.
    ///
    /// Only user classes that implement `Cloneable` produce a clone helper;
    /// built-in collections and strings use the runtime's default IncRef path.
    pub(crate) fn elem_clone_addr_for_shape(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        shape: ElementShape,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let ElementShape::UserClass(name) = shape else {
            return Ok(None);
        };
        if !Self::class_implements_cloneable(name, type_definitions) {
            return Ok(None);
        }
        Ok(Some(Self::get_custom_clone_thunk_addr(
            builder, ctx, name, ptr_type,
        )?))
    }

    /// Sets `elem_drop_fn` on `list_ptr` based on the declared element type.
    ///
    /// Used when an empty `List<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    pub(crate) fn emit_list_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)? {
            Self::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Extracts the element expression from a Set TypeKind.
    pub(crate) fn set_elem_expr(kind: &TypeKind) -> Option<&crate::ast::expression::Expression> {
        match kind {
            TypeKind::Set(e) => Some(e),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Set) =>
            {
                args.first()
            }
            _ => None,
        }
    }

    /// Sets `elem_drop_fn` on `set_ptr` based on the declared element type.
    ///
    /// Used when an empty `Set<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    pub(crate) fn emit_set_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)? {
            Self::call_rt_set_set_elem_drop_fn(builder, ctx, set_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_clone_fn` on `list_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_list_drop_fn_for_elem_kind` but for the clone
    /// side. Called on the empty-constructor path where `translate_rvalue` has no
    /// operands to inspect.
    pub(crate) fn emit_list_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
            Self::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_clone_fn` on `set_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_set_drop_fn_for_elem_kind` but for the clone
    /// side.
    pub(crate) fn emit_set_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
            Self::call_rt_set_set_elem_clone_fn(builder, ctx, set_ptr, addr)?;
        }
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

    /// Returns the element `Type` of a collection type (Array or List), or `None`.
    ///
    /// Unlike `resolve_collection_elem_type` which returns `&TypeKind`, this returns
    /// the full `&Type` so callers can chain through multiple projection levels.
    pub(crate) fn resolve_collection_elem_type_as_type(base_type: &Type) -> Option<&Type> {
        match &base_type.kind {
            TypeKind::Array(elem_ty_expr, _) | TypeKind::List(elem_ty_expr) => {
                if let ExpressionKind::Type(ty, _) = &elem_ty_expr.node {
                    Some(ty.as_ref())
                } else {
                    None
                }
            }
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) =>
            {
                args.first().and_then(|arg| {
                    if let ExpressionKind::Type(ty, _) = &arg.node {
                        Some(ty.as_ref())
                    } else {
                        None
                    }
                })
            }
            _ => None,
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

    /// Extracts the element expression from a List or Array TypeKind.
    /// Handles both canonical variants (`TypeKind::List(e)`, `TypeKind::Array(e, _)`)
    /// and the normalised `TypeKind::Custom` form where
    /// `BuiltinCollectionKind::from_name` returns `List` or `Array`.
    pub(crate) fn collection_elem_expr(
        kind: &TypeKind,
    ) -> Option<&crate::ast::expression::Expression> {
        match kind {
            TypeKind::List(e) | TypeKind::Array(e, _) => Some(e),
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name),
                    Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
                ) =>
            {
                args.first()
            }
            _ => None,
        }
    }

    /// Extracts the value expression from a Map TypeKind.
    /// Handles both canonical `TypeKind::Map(_, v)` and the normalised
    /// `TypeKind::Custom` form where `BuiltinCollectionKind::from_name`
    /// returns `Map` (with `[_, v]` as generic args).
    fn map_val_expr(kind: &TypeKind) -> Option<&crate::ast::expression::Expression> {
        match kind {
            TypeKind::Map(_, v) => Some(v),
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Map) =>
            {
                args.get(1)
            }
            _ => None,
        }
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
    ) -> Result<(), CodegenError> {
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
    ) -> Result<(), CodegenError> {
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
    ) -> Result<(), CodegenError> {
        // Resolve type aliases before dispatching so that e.g.
        // `type IntArray is [int; 2]` correctly frees via rt_array_free.
        let resolved = Self::resolve_alias(kind, type_ctx.type_definitions);
        let kind = resolved.unwrap_or(kind);

        if Self::is_map_type(kind) {
            Self::emit_drop_map(builder, ctx, kind, ptr, type_ctx)
        } else if Self::is_set_type(kind) {
            Self::call_rt_set_free(builder, ctx, ptr)
        } else if Self::is_list_type(kind) {
            Self::emit_drop_list_or_array(builder, ctx, kind, ptr, type_ctx, true)
        } else if Self::is_collection_type(kind) {
            Self::emit_drop_list_or_array(builder, ctx, kind, ptr, type_ctx, false)
        } else if let TypeKind::Tuple(element_exprs) = kind {
            Self::emit_drop_tuple(builder, ctx, kind, element_exprs, ptr, header_ptr, type_ctx)
        } else if let TypeKind::Option(inner) = kind {
            Self::emit_drop_option(builder, ctx, inner, ptr, header_ptr, type_ctx)
        } else if kind == &TypeKind::String {
            // miri_rt_string_free takes the payload pointer (not header) — it
            // calls free_with_rc internally.
            Self::call_rt_string_free(builder, ctx, ptr)
        } else if let TypeKind::Custom(name, _) = kind {
            Self::emit_drop_custom(builder, ctx, name, ptr, header_ptr, type_ctx)
        } else if matches!(kind, TypeKind::Function(_)) {
            Self::emit_drop_closure(builder, ctx, ptr, header_ptr, type_ctx)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// DecRef managed values stored in a `MiriMap`, then free the map struct.
    /// Map layout: `[states][keys][values][len][capacity]...` (ptr-sized fields).
    fn emit_drop_map(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if let Some(val_expr) = Self::map_val_expr(kind) {
            if let ExpressionKind::Type(val_ty, _) = &val_expr.node {
                if is_field_managed(&val_ty.kind) {
                    let ptr_type = type_ctx.ptr_type;
                    let ptr_size = ptr_type.bytes() as i32;
                    let states = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                    let values = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), ptr, 2 * ptr_size);
                    let capacity = builder
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
    }

    /// DecRef managed elements before freeing a `MiriList` or `MiriArray`.
    ///
    /// Both layouts begin `[data: ptr][len/elem_count: ptr]...`. For Array, also
    /// zeros `elem_drop_fn` (slot 3 * ptr_size) so the runtime's free path does
    /// not run the per-element decref a second time.
    fn emit_drop_list_or_array(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
        is_list: bool,
    ) -> Result<(), CodegenError> {
        if let Some(inner_expr) = Self::collection_elem_expr(kind) {
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
                    if !is_list {
                        // Array: zero elem_drop_fn (slot 3) so the runtime's free
                        // path does not call decref a second time on already-
                        // dropped elements.
                        let zero = builder.ins().iconst(ptr_type, 0);
                        builder
                            .ins()
                            .store(MemFlags::new(), zero, ptr, 3 * ptr_size);
                    }
                }
            }
        }
        if is_list {
            Self::call_rt_list_free(builder, ctx, ptr)
        } else {
            Self::call_rt_array_free(builder, ctx, ptr)
        }
    }

    /// DecRef each managed field of a tuple, then `free()` the RC block.
    /// Layout: `[elem_count: ptr][field0][field1]...` (payload_ptr = `ptr`).
    #[allow(clippy::too_many_arguments)]
    fn emit_drop_tuple(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        element_exprs: &[crate::ast::expression::Expression],
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let tuple_type = kind.clone();
        let managed_fields: Vec<(i32, TypeKind)> = element_exprs
            .iter()
            .enumerate()
            .filter_map(|(i, expr)| {
                let ExpressionKind::Type(ty, _) = &expr.node else {
                    return None;
                };
                if !is_field_managed(&ty.kind) {
                    return None;
                }
                let (offset, _) =
                    layout::field_layout(&tuple_type, i, type_ctx.type_definitions, ptr_type);
                Some((offset, ty.kind.clone()))
            })
            .collect();
        for (offset, field_kind) in managed_fields {
            let field_ptr = builder.ins().load(ptr_type, MemFlags::new(), ptr, offset);
            Self::emit_decref_value(builder, ctx, &field_kind, field_ptr, type_ctx)?;
        }
        Self::call_libc_free(builder, ctx, header_ptr)
    }

    /// DecRef an Option's inner value (when managed), then free the RC block.
    fn emit_drop_option(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        inner: &Type,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if is_field_managed(&inner.kind) {
            let ptr_type = type_ctx.ptr_type;
            let cl_inner_ty =
                crate::codegen::cranelift::types::translate_type_kind(&inner.kind, ptr_type);
            let inner_ptr = builder.ins().load(cl_inner_ty, MemFlags::new(), ptr, 0);
            Self::emit_decref_value(builder, ctx, &inner.kind, inner_ptr, type_ctx)?;
        }
        Self::call_libc_free(builder, ctx, header_ptr)
    }

    /// Drop a custom struct/class/enum: dispatch through the type-specific
    /// `__drop_TypeName` thunk when it carries managed fields or a user-defined
    /// drop hook; otherwise free the RC block directly.
    fn emit_drop_custom(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let needs_thunk = Self::has_managed_fields(name, type_ctx.type_definitions)
            || Self::type_has_user_drop(name, type_ctx.type_definitions);
        if needs_thunk {
            Self::call_drop_thunk(builder, ctx, name, ptr, type_ctx.ptr_type)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// Drop a closure: invoke its `dtor_ptr` (when non-null) to DecRef captures,
    /// then decrement the closure-balance counter and free the closure struct.
    /// Layout: `payload[0]=fn_ptr, payload[1]=dtor_ptr, payload[2+i]=cap_i`.
    fn emit_drop_closure(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;
        let dtor_ptr = builder
            .ins()
            .load(ptr_type, MemFlags::new(), ptr, ptr_size as i32);
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            dtor_ptr,
            null,
        );
        let dtor_block = builder.create_block();
        let after_dtor = builder.create_block();
        builder
            .ins()
            .brif(is_null, after_dtor, &[], dtor_block, &[]);
        builder.switch_to_block(dtor_block);
        builder.seal_block(dtor_block);
        let mut dtor_sig = cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
        dtor_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let dtor_sig_ref = builder.import_signature(dtor_sig);
        builder.ins().call_indirect(dtor_sig_ref, dtor_ptr, &[ptr]);
        builder.ins().jump(after_dtor, &[]);
        builder.switch_to_block(after_dtor);
        builder.seal_block(after_dtor);
        Self::call_rt_closure_free_track(builder, ctx)?;
        Self::call_libc_free(builder, ctx, header_ptr)
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
    ) -> Result<(), CodegenError> {
        let Some(def) = type_ctx.type_definitions.get(type_name) else {
            return Ok(());
        };
        match def {
            TypeDefinition::Struct(struct_def) => {
                let managed_fields: Vec<(usize, TypeKind)> = struct_def
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, ty, _))| is_field_managed(&ty.kind))
                    .map(|(idx, (_, ty, _))| (idx, ty.kind.clone()))
                    .collect();
                Self::emit_struct_like_field_decrefs(
                    builder,
                    ctx,
                    type_name,
                    &managed_fields,
                    payload_ptr,
                    type_ctx,
                )
            }
            TypeDefinition::Enum(enum_def) => {
                Self::emit_enum_drop(builder, ctx, enum_def, payload_ptr, type_ctx)
            }
            TypeDefinition::Class(class_def) => {
                use crate::type_checker::context::collect_class_fields_all;
                let all_fields = collect_class_fields_all(class_def, type_ctx.type_definitions);
                let managed_fields: Vec<(usize, TypeKind)> = all_fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, fi))| is_field_managed(&fi.ty.kind))
                    .map(|(idx, (_, fi))| (idx, fi.ty.kind.clone()))
                    .collect();
                Self::emit_struct_like_field_decrefs(
                    builder,
                    ctx,
                    type_name,
                    &managed_fields,
                    payload_ptr,
                    type_ctx,
                )
            }
            TypeDefinition::Trait(_) | TypeDefinition::Alias(_) | TypeDefinition::Generic(_) => {
                Ok(())
            }
        }
    }

    /// Emit `DecRef` for every managed field of a struct- or class-shaped
    /// type. `managed_fields` carries the (full-field-list) index and field
    /// type kind; offsets are resolved through `layout::field_layout` so
    /// inherited fields land at the right slot.
    fn emit_struct_like_field_decrefs(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        managed_fields: &[(usize, TypeKind)],
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let custom_kind = TypeKind::Custom(type_name.to_string(), None);
        for (field_idx, field_kind) in managed_fields {
            let (offset, _cl_ty) = layout::field_layout(
                &custom_kind,
                *field_idx,
                type_ctx.type_definitions,
                ptr_type,
            );
            let field_ptr = builder
                .ins()
                .load(ptr_type, MemFlags::new(), payload_ptr, offset);
            Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
        }
        Ok(())
    }

    /// Drop an enum payload. Reads the discriminant at offset 0 and, for each
    /// variant that carries managed fields, emits a guarded block that
    /// DecRefs the variant's fields when the discriminant matches.
    fn emit_enum_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        enum_def: &crate::type_checker::context::EnumDefinition,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let disc = builder
            .ins()
            .load(ptr_type, MemFlags::new(), payload_ptr, 0);

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
                let field_ptr =
                    builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), payload_ptr, field_offset);
                Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
            }
            builder.ins().jump(merge_block, &[]);
            builder.seal_block(drop_block);
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
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
    ) -> Result<(), CodegenError> {
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

        let elem_type_kind = Self::resolve_collection_elem_type(base_type, ptr_type);
        let cl_elem_ty =
            crate::codegen::cranelift::types::translate_type_kind(elem_type_kind, ptr_type);
        let elem_size = cl_elem_ty.bytes() as i64;

        // Tuples store `[count][field0][field1]...` at `base_value`; the
        // `data` field is implicit (fields start right after the count slot).
        // Also covers `Custom("Tuple", ...)` rooted inside class bodies.
        let is_tuple_type = matches!(&base_type.kind, TypeKind::Tuple(_))
            || matches!(&base_type.kind, TypeKind::Custom(name, _) if name == "Tuple");

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

        // Compute element address
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(data_ptr, byte_offset);
        // DecRef the old managed element before overwriting
        Self::emit_managed_elem_decref(builder, ctx, elem_addr, elem_type_kind, ptr_type)?;
        builder.ins().store(MemFlags::new(), value, elem_addr, 0);

        builder.seal_block(panic_block);
        builder.seal_block(cont_block);

        Ok(())
    }

    /// Decrements the RC of the existing element at `elem_addr` when the element
    /// type is a managed heap object (String, List, Array, Set, Map, or user-defined class).
    ///
    /// Called by `translate_collection_index_write` before the new value is stored
    /// so that overwriting an existing slot does not leak the old value.
    fn emit_managed_elem_decref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_addr: Value,
        elem_type_kind: &TypeKind,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_type_kind);
        let builtin_decref = match shape {
            ElementShape::String => Some(rt::STRING_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::List) => Some(rt::LIST_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Array) => Some(rt::ARRAY_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Set) => Some(rt::SET_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Map) => Some(rt::MAP_DECREF_ELEMENT),
            ElementShape::UserClass(_) | ElementShape::Other => None,
        };
        if let Some(name) = builtin_decref {
            let old_val = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
            Self::call_cached_func(
                builder,
                ctx.module,
                &mut ctx.cached_funcs,
                CallSite {
                    name,
                    param_types: &[ptr_type],
                    return_types: &[],
                    args: &[old_val],
                },
            )?;
            return Ok(());
        }
        let ElementShape::UserClass(class_name) = shape else {
            // Primitives need no decref.
            return Ok(());
        };
        let mut decref_name = String::with_capacity(9 + class_name.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(class_name);
        let old_val = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
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
        builder.ins().call(local_func, &[old_val]);
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
    ) -> Result<(), CodegenError> {
        let mut thunk_name = String::with_capacity(7 + type_name.len());
        thunk_name.push_str("__drop_");
        thunk_name.push_str(type_name);
        let mut sig = Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = ctx
            .module
            .declare_function(&thunk_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(thunk_name.clone(), e.to_string()))?;
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
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut func_name = String::with_capacity(7 + type_name.len());
        func_name.push_str("__drop_");
        func_name.push_str(type_name);
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&func_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_drop_body(
            module,
            ctx,
            &mut builder_ctx,
            type_name,
            type_definitions,
            ptr_type,
            call_conv,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(func_name, e.to_string()))?;
        ctx.clear();

        // Generate __decref_TypeName: the RC-decrement wrapper used as
        // elem_drop_fn for collections holding custom-type elements.
        Self::generate_decref_function(module, ctx, isa, type_name)
    }

    /// Emit the body of `__drop_TypeName(ptr)`:
    ///   1. invoke user-defined `fn drop(self)` when the type defines one,
    ///   2. DecRef every managed field via `emit_struct_drop`,
    ///   3. free the RC allocation via libc `free`.
    #[allow(clippy::too_many_arguments)]
    fn emit_drop_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            type_definitions,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
        };

        let type_has_drop = match type_definitions.get(type_name) {
            Some(TypeDefinition::Struct(sd)) => sd.has_drop,
            Some(TypeDefinition::Class(cd)) => cd.has_drop,
            _ => false,
        };
        if type_has_drop {
            let mut user_drop_name = String::with_capacity(type_name.len() + 5);
            user_drop_name.push_str(type_name);
            user_drop_name.push_str("_drop");
            // ABI mirrors lower_class_method: (self: ptr, allocator: ptr) -> void
            let mut user_sig = Signature::new(call_conv);
            user_sig.params.push(AbiParam::new(ptr_type));
            user_sig.params.push(AbiParam::new(ptr_type));
            let user_drop_id = module_ctx
                .module
                .declare_function(&user_drop_name, Linkage::Import, &user_sig)
                .map_err(|e| CodegenError::declare_function(user_drop_name, e.to_string()))?;
            let local_user_drop = module_ctx
                .module
                .declare_func_in_func(user_drop_id, builder.func);
            let zero = builder.ins().iconst(ptr_type, 0);
            builder.ins().call(local_user_drop, &[ptr, zero]);
        }

        Self::emit_struct_drop(&mut builder, &mut module_ctx, type_name, ptr, &type_ctx)?;

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        Self::call_libc_free(&mut builder, &mut module_ctx, header_ptr)?;

        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Generates `__decref_{type_name}(ptr)` in the given module.
    ///
    /// Emits the RC-decrement pattern:
    ///   1. Guard: skip if ptr is null.
    ///   2. Guard: skip if RC < 0 (immortal).
    ///   3. Decrement RC.
    ///   4. If RC reaches zero, call `__drop_{type_name}(ptr)`.
    ///
    /// Used as `elem_drop_fn` for List/Set/Map holding custom-type elements so
    /// that mutation operations (clear, remove_at, remove) properly DecRef
    /// each removed instance.
    fn generate_decref_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut decref_name = String::with_capacity(9 + type_name.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(type_name);
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&decref_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_decref_body(module, ctx, &mut builder_ctx, type_name, ptr_type, &sig)?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(decref_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `__decref_TypeName`: null guard → immortal guard →
    /// decrement RC → when RC hits zero call `__drop_TypeName(ptr)` → return.
    fn emit_decref_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        ptr_type: cl_types::Type,
        sig: &Signature,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        // Null guard.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        // Load RC + check immortal flag (high bit set).
        builder.switch_to_block(rc_block);
        builder.seal_block(rc_block);
        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);
        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        // Decrement RC; branch to `__drop` thunk when it reaches zero.
        builder.switch_to_block(dec_block);
        builder.seal_block(dec_block);
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

        // `__drop_TypeName(ptr)` call site.
        builder.switch_to_block(free_block);
        builder.seal_block(free_block);
        let mut drop_name = String::with_capacity(7 + type_name.len());
        drop_name.push_str("__drop_");
        drop_name.push_str(type_name);
        let drop_func_id = module
            .declare_function(&drop_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(drop_name.clone(), e.to_string()))?;
        let local_drop = module.declare_func_in_func(drop_func_id, builder.func);
        builder.ins().call(local_drop, &[ptr]);
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        builder.ins().return_(&[]);
        builder.finalize();
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

    /// Generates `__clone_{type_name}(ptr) -> ptr` for each concrete class that
    /// implements `Cloneable`.
    ///
    /// This function is used as `elem_clone_fn` in Array/List/Set so that
    /// `miri_rt_XXX_clone` produces independent element copies instead of just
    /// IncRef-ing shared pointers.  It delegates to the user's compiled `clone()`
    /// method (`{TypeName}_clone`), which already encodes any deep-copy logic.
    ///
    /// Only generated for concrete (non-generic, non-abstract) classes whose
    /// `traits` list includes `"Cloneable"` (checked with inheritance walk).
    pub(crate) fn generate_clone_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        // Only generate for concrete classes that implement Cloneable somewhere in their
        // hierarchy. Abstract classes may have an abstract clone() with no compiled body;
        // generating a thunk for them would reference an undefined symbol at link time.
        match type_definitions.get(type_name) {
            Some(TypeDefinition::Class(cd)) if cd.is_abstract => return Ok(()),
            _ => {}
        }
        if !Self::class_implements_cloneable(type_name, type_definitions) {
            return Ok(());
        }

        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut clone_name = String::with_capacity(9 + type_name.len());
        clone_name.push_str("__clone_");
        clone_name.push_str(type_name);

        // Signature: (ptr: *TypeName) -> *TypeName
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns.push(AbiParam::new(ptr_type));

        let func_id = module
            .declare_function(&clone_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(clone_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_clone_body(
            module,
            ctx,
            &mut builder_ctx,
            type_name,
            type_definitions,
            ptr_type,
            call_conv,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(clone_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `__clone_TypeName(ptr)`: null guard → call the
    /// user-defined `clone()` resolved through the inheritance chain →
    /// return the result.
    #[allow(clippy::too_many_arguments)]
    fn emit_clone_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        // Null guard: return null if ptr is null.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let null_ret_block = builder.create_block();
        let call_block = builder.create_block();
        builder
            .ins()
            .brif(is_null, null_ret_block, &[], call_block, &[]);

        builder.switch_to_block(null_ret_block);
        builder.seal_block(null_ret_block);
        builder.ins().return_(&[null]);

        builder.switch_to_block(call_block);
        builder.seal_block(call_block);

        // Resolve clone() through inheritance (applies concrete-caller / abstract-definer rule).
        let clone_method_name = Self::resolve_clone_method_name(type_name, type_definitions);
        let mut user_clone_sig = Signature::new(call_conv);
        user_clone_sig.params.push(AbiParam::new(ptr_type)); // self
        user_clone_sig.params.push(AbiParam::new(ptr_type)); // allocator
        user_clone_sig.returns.push(AbiParam::new(ptr_type));

        let user_clone_id = module
            .declare_function(&clone_method_name, Linkage::Import, &user_clone_sig)
            .map_err(|e| CodegenError::declare_function(clone_method_name, e.to_string()))?;
        let local_fn = module.declare_func_in_func(user_clone_id, builder.func);
        let zero = builder.ins().iconst(ptr_type, 0);
        let inst = builder.ins().call(local_fn, &[ptr, zero]);
        let result = builder.inst_results(inst)[0];
        builder.ins().return_(&[result]);

        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Resolves the mangled name of the `clone()` method for `type_name`.
    ///
    /// Walks the inheritance chain to find where `clone()` is defined.  The
    /// concrete-caller / abstract-definer rule is applied: if the defining class
    /// is abstract, the caller's name is used instead (matching how
    /// `resolve_inherited_method` in `mir::lowering::dispatch` mangles the call).
    fn resolve_clone_method_name(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> String {
        crate::mir::lowering::dispatch::resolve_inherited_method(
            type_definitions,
            type_name,
            "clone",
        )
        .map(|(defining, _)| format!("{defining}_clone"))
        .unwrap_or_else(|| format!("{type_name}_clone"))
    }

    /// Returns true if `type_name` (or any ancestor class) lists `"Cloneable"` in its traits.
    pub(crate) fn class_implements_cloneable(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        let mut current = type_name.to_string();
        loop {
            match type_definitions.get(&current) {
                Some(TypeDefinition::Class(cd)) => {
                    if cd
                        .traits
                        .iter()
                        .any(|t| t == crate::ast::types::CLONEABLE_TRAIT_NAME)
                    {
                        return true;
                    }
                    match &cd.base_class {
                        Some(base) => current = base.clone(),
                        None => return false,
                    }
                }
                Some(
                    TypeDefinition::Struct(_)
                    | TypeDefinition::Enum(_)
                    | TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_)
                    | TypeDefinition::Trait(_),
                )
                | None => return false,
            }
        }
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
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        let call_conv = isa.default_call_conv();

        let classes = Self::collect_classes_needing_vtable(type_definitions);
        for (class_name, _) in &classes {
            let vtable_methods = Self::collect_vtable_methods(class_name, type_definitions);
            if vtable_methods.is_empty() {
                continue;
            }
            Self::emit_vtable_for_class(
                module,
                class_name,
                &vtable_methods,
                type_definitions,
                ptr_type,
                ptr_size,
                call_conv,
            )?;
        }
        Ok(())
    }

    /// Return the concrete (non-abstract) classes that participate in virtual
    /// dispatch, sorted by class name for deterministic codegen output.
    /// Generic classes are included — their methods are compiled once with
    /// `T` treated as a pointer-sized opaque slot.
    fn collect_classes_needing_vtable(
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Vec<(&str, &crate::type_checker::context::ClassDefinition)> {
        use crate::type_checker::context::class_needs_vtable;
        let mut classes: Vec<(&str, &crate::type_checker::context::ClassDefinition)> =
            type_definitions
                .iter()
                .filter_map(|(name, def)| {
                    let TypeDefinition::Class(cd) = def else {
                        return None;
                    };
                    if !cd.is_abstract && class_needs_vtable(name, type_definitions) {
                        Some((name.as_str(), cd))
                    } else {
                        None
                    }
                })
                .collect();
        classes.sort_unstable_by_key(|(name, _)| *name);
        classes
    }

    /// Collect the vtable's method-name slots for `class_name`. Walks the
    /// abstract-ancestor chain (root → leaf order so base methods come first)
    /// and merges in trait-required methods from every implemented trait.
    /// Results are sorted alphabetically for deterministic slot indices.
    fn collect_vtable_methods<'td>(
        class_name: &str,
        type_definitions: &'td HashMap<String, TypeDefinition>,
    ) -> Vec<&'td str> {
        use crate::type_checker::context::collect_trait_vtable_methods;

        // 1. Walk inheritance chain, recording every abstract ancestor.
        let mut abstract_chain: Vec<&str> = Vec::new();
        let mut current: &str = class_name;
        loop {
            match type_definitions.get(current) {
                Some(TypeDefinition::Class(cd)) if cd.is_abstract => {
                    abstract_chain.push(current);
                    match &cd.base_class {
                        Some(base) => current = base,
                        None => break,
                    }
                }
                Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                    Some(base) => current = base,
                    None => break,
                },
                _ => break,
            }
        }

        // 2. Collect methods from abstract ancestors (base-first ordering).
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut vtable_methods: Vec<&str> = Vec::new();
        for ancestor in abstract_chain.iter().rev() {
            if let Some(TypeDefinition::Class(cd)) = type_definitions.get(*ancestor) {
                for (method_name, method_info) in &cd.methods {
                    if !method_info.is_constructor && !seen.contains(method_name.as_str()) {
                        seen.insert(method_name.as_str());
                        vtable_methods.push(method_name.as_str());
                    }
                }
            }
        }

        // 3. Merge in trait-required methods from the class and every ancestor.
        let mut walk: &str = class_name;
        while let Some(TypeDefinition::Class(cd)) = type_definitions.get(walk) {
            for trait_name in &cd.traits {
                for m in collect_trait_vtable_methods(type_definitions, trait_name) {
                    if !seen.contains(m) {
                        seen.insert(m);
                        vtable_methods.push(m);
                    }
                }
            }
            match &cd.base_class {
                Some(b) => walk = b,
                None => break,
            }
        }

        vtable_methods.sort();
        vtable_methods
    }

    /// Declare-and-define one `__vtable_{class_name}` data symbol with one
    /// pointer slot per method (resolved through the class's inheritance chain
    /// via `resolve_vtable_method`). Method symbols that are not yet declared
    /// in the module are imported with a placeholder signature.
    #[allow(clippy::too_many_arguments)]
    fn emit_vtable_for_class(
        module: &mut ObjectModule,
        class_name: &str,
        vtable_methods: &[&str],
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cranelift_codegen::ir::Type,
        ptr_size: u32,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        use cranelift_module::Linkage;
        let vtable_size = (vtable_methods.len() * ptr_size as usize) as u32;

        let mut vtable_sym = String::with_capacity(9 + class_name.len());
        vtable_sym.push_str("__vtable_");
        vtable_sym.push_str(class_name);
        let vtable_data_id = module
            .declare_data(&vtable_sym, Linkage::Export, false, false)
            .map_err(|e| CodegenError::declare_function(vtable_sym.clone(), e.to_string()))?;

        let mut desc = cranelift_module::DataDescription::new();
        desc.set_align(ptr_size as u64);
        desc.define(vec![0u8; vtable_size as usize].into_boxed_slice());

        for (slot_idx, method_name) in vtable_methods.iter().enumerate() {
            let Some(func_name) =
                Self::resolve_vtable_method(class_name, method_name, type_definitions)
            else {
                continue;
            };
            let func_id = Self::vtable_slot_func_id(module, &func_name, ptr_type, call_conv)?;
            let func_ref = module.declare_func_in_data(func_id, &mut desc);
            let slot_offset = (slot_idx * ptr_size as usize) as u32;
            desc.write_function_addr(slot_offset, func_ref);
        }

        module
            .define_data(vtable_data_id, &desc)
            .map_err(|e| CodegenError::define_function(vtable_sym.clone(), e.to_string()))
    }

    /// Look up `func_name` in the module; declare it as `Linkage::Import` with
    /// a placeholder `(ptr) -> ()` signature when missing. Covers abstract-
    /// base concrete methods only invoked through vtable dispatch.
    fn vtable_slot_func_id(
        module: &mut ObjectModule,
        func_name: &str,
        ptr_type: cranelift_codegen::ir::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        use cranelift_module::{FuncOrDataId, Linkage};
        if let Some(FuncOrDataId::Func(id)) = module.get_name(func_name) {
            return Ok(id);
        }
        let mut sig = cranelift_codegen::ir::Signature::new(call_conv);
        sig.params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        module
            .declare_function(func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.to_string(), e.to_string()))
    }

    /// Resolve which function implements `method_name` for `class_name` via inheritance.
    /// Returns `"ClassName_methodName"` for the first class in the chain that defines it,
    /// or `"TraitName_methodName"` if the method is a default trait implementation.
    fn resolve_vtable_method(
        class_name: &str,
        method_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        let mut current: &str = class_name;
        let mut all_traits: Vec<&str> = Vec::new();
        while let Some(TypeDefinition::Class(cd)) = type_definitions.get(current) {
            if let Some(method) = cd.methods.get(method_name) {
                // Only use this implementation if it has a body (not abstract).
                if !method.is_abstract {
                    let mut mangled = String::with_capacity(current.len() + 1 + method_name.len());
                    mangled.push_str(current);
                    mangled.push('_');
                    mangled.push_str(method_name);
                    return Some(mangled);
                }
            }
            all_traits.extend(cd.traits.iter().map(|s| s.as_str()));
            match &cd.base_class {
                Some(base) => current = base,
                None => break,
            }
        }
        // Fall back to a default trait method implementation.
        let mut visited = std::collections::HashSet::new();
        let mut trait_stack = all_traits;
        while let Some(t_name) = trait_stack.pop() {
            if !visited.insert(t_name) {
                continue;
            }
            if let Some(TypeDefinition::Trait(td)) = type_definitions.get(t_name) {
                if let Some(method) = td.methods.get(method_name) {
                    if !method.is_abstract {
                        let mut mangled =
                            String::with_capacity(t_name.len() + 1 + method_name.len());
                        mangled.push_str(t_name);
                        mangled.push('_');
                        mangled.push_str(method_name);
                        return Some(mangled);
                    }
                }
                trait_stack.extend(td.parent_traits.iter().map(|s| s.as_str()));
            }
        }
        None
    }

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
        let mut module_ctx = empty_module_ctx(module, &mut string_literals);
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

    /// Returns true if a named Custom type has at least one managed field.
    ///
    /// Used to decide whether to call `__drop_TypeName` (when there are managed
    /// fields to clean up) or just `libc::free` (when all fields are primitives).
    /// Returns true if the type defines `fn drop(self)` (user-controlled teardown).
    pub(crate) fn type_has_user_drop(
        name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match type_defs.get(name) {
            Some(TypeDefinition::Struct(def)) => def.has_drop,
            Some(TypeDefinition::Class(def)) => def.has_drop,
            _ => false,
        }
    }

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
        let list_custom = TypeKind::Custom("List".to_string(), Some(vec![expr_ty(TypeKind::Int)]));
        assert!(matches!(
            FunctionTranslator::classify_element_shape(&list_custom),
            ElementShape::Builtin(BuiltinCollectionKind::List)
        ));
    }

    #[test]
    fn classify_element_shape_custom_user_class() {
        let user = TypeKind::Custom("MyClass".to_string(), None);
        match FunctionTranslator::classify_element_shape(&user) {
            ElementShape::UserClass(name) => assert_eq!(name, "MyClass"),
            other => panic!("expected UserClass, got {:?}", other),
        }
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
