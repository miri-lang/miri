// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::translator::{
    needs_out_pointer, CallSite, ElementShape, FunctionTranslator, ModuleCtx, TypeCtx,
};
use crate::codegen::cranelift::types::{translate_type, translate_type_kind};
use crate::error::CodegenError;
use crate::mir::{
    AggregateKind, BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
use cranelift_codegen::ir::{
    condcodes::IntCC, AbiParam, Block, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, TrapCode,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

/// Output of `prepare_call_args`: per-arg Cranelift values, the partially-built
/// call signature (params filled, returns appended later by the caller), and
/// the list of scalar-out stack slots paired with their caller-side `Local`s
/// for post-call writeback.
type PreparedCallArgs = (
    Vec<cranelift_codegen::ir::Value>,
    Signature,
    Vec<(cranelift_codegen::ir::Value, Local)>,
);

impl<'a> FunctionTranslator<'a> {
    /// Translate a MIR statement.
    pub(crate) fn translate_statement(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        stmt: &Statement,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        match &stmt.kind {
            StatementKind::IncRef(place) => {
                Self::translate_inc_ref(builder, ctx, place, locals, type_ctx)
            }
            StatementKind::DecRef(place) => {
                Self::translate_dec_ref(builder, ctx, place, locals, type_ctx)
            }
            StatementKind::Dealloc(place) => {
                Self::translate_dealloc(builder, ctx, place, locals, type_ctx)
            }
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                Self::translate_assign(builder, ctx, place, rvalue, locals, type_ctx)
            }
            StatementKind::StorageDead(place) => {
                Self::translate_gpu_buffer_release(builder, ctx, place, body)
            }
            StatementKind::Nop | StatementKind::StorageLive(_) => Ok(()),
        }
    }

    /// Frees the persistent device buffer of a `gpu`-resident local at its
    /// `StorageDead` (scope exit). The handle id is a compile-time constant on
    /// the local's decl; a non-`gpu` local carries no handle and this is a
    /// no-op. `miri_gpu_release` is idempotent, so a `gpu`-to-`gpu` move whose
    /// handle is shared by two locals releases it once and the second call is a
    /// harmless no-op.
    fn translate_gpu_buffer_release(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        body: &Body,
    ) -> Result<(), CodegenError> {
        // Compiler-synthesized GPU intrinsic (never a `runtime "gpu" fn` in
        // stdlib); codegen declares the import on demand, matching the
        // `miri_gpu_readback` / `miri_gpu_launch_inline` convention. The handle
        // id is a `u64`, so the runtime ABI is a single `I64` parameter.
        const RELEASE_FN: &str = "miri_gpu_release";

        let decl = &body.local_decls[place.local.0];
        let Some(handle) = decl.device_handle else {
            return Ok(());
        };
        // A borrowed handle (residency-specialized parameter) launches on the
        // caller's buffer but does not own it; the caller's binding frees it.
        // This flag is the sole gate preventing a borrowed parameter from
        // releasing a caller-owned buffer (a double-free); it is set only by
        // `stamp_residency_param_handles`, which every residency specialization
        // routes through.
        if decl.device_handle_borrowed {
            return Ok(());
        }
        let handle_val = builder
            .ins()
            .iconst(cranelift_codegen::ir::types::I64, handle.0 as i64);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: RELEASE_FN,
                param_types: &[cranelift_codegen::ir::types::I64],
                return_types: &[],
                args: &[handle_val],
            },
        )?;
        Ok(())
    }

    /// `StatementKind::IncRef`: bump the RC slot at `payload - ptr_size`,
    /// unless the pointer is null (uninitialized local) or the high bit of
    /// the RC is set (immortal/constant object).
    fn translate_inc_ref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        builder.switch_to_block(rc_block);
        let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
        let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);

        let is_immortal = builder.ins().icmp_imm(IntCC::SignedLessThan, rc, 0);
        let then_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], then_block, &[]);

        builder.switch_to_block(then_block);
        let new_rc = builder.ins().iadd_imm(rc, 1);
        builder.ins().store(MemFlags::new(), new_rc, header_ptr, 0);
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(rc_block);
        builder.seal_block(then_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(())
    }

    /// `StatementKind::DecRef`: decrement RC; when it reaches zero, run
    /// `emit_type_drop` for the resolved place kind. Skips null pointers
    /// (uninitialized) and immortal objects (high RC bit set).
    ///
    /// Delegates to `emit_decref_value`, which carries the canonical
    /// null-guard / immortal-guard / decrement / zero-check chain. The
    /// only place-specific work is resolving the projected type kind and
    /// reading the pointer value.
    fn translate_dec_ref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let place_kind = Self::resolve_projected_type_kind(place, type_ctx);
        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;
        Self::emit_decref_value(builder, ctx, &place_kind, ptr, type_ctx)
    }

    /// `StatementKind::Dealloc`: unconditional cleanup — the caller has
    /// already determined this value's RC chain is done. Skips null pointers.
    fn translate_dealloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let place_kind_cow = Self::resolve_projected_type_kind(place, type_ctx);
        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
        let dealloc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder
            .ins()
            .brif(is_null, merge_block, &[], dealloc_block, &[]);

        builder.switch_to_block(dealloc_block);
        let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
        Self::emit_type_drop(builder, ctx, &place_kind_cow, ptr, header_ptr, type_ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(dealloc_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(())
    }

    /// `StatementKind::Assign` / `Reassign`: translate the rvalue, cast to
    /// the destination's declared type if widths differ, and store. After an
    /// empty `Set<T>()` constructor, also register elem_drop_fn / elem_clone_fn
    /// from the destination type's element annotation.
    fn translate_assign(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;

        // Compute destination type for potential use in operand translation.
        let dest_ty = &type_ctx.local_types[place.local.0];
        let dest_kind_to_cast = if place.projection.is_empty() {
            dest_ty.kind.clone()
        } else {
            Self::resolve_projected_type_kind(place, type_ctx)
        };

        // Pass the destination type when translating Rvalue::Use and Rvalue::Aggregate
        // so field loads and aggregate payloads can resolve to the correct width for
        // enum/Option payloads.
        let expected_ty_for_rvalue = if matches!(rvalue, Rvalue::Use(_) | Rvalue::Aggregate(..)) {
            Some(dest_ty as &_)
        } else {
            None
        };

        let mut value = Self::translate_rvalue(
            builder,
            ctx,
            rvalue,
            locals,
            type_ctx,
            expected_ty_for_rvalue,
        )?;

        if place.projection.is_empty() {
            value = Self::copy_bound_inline_vec(builder, ctx, rvalue, value, type_ctx)?;
        }

        let dest_cl_ty = translate_type_kind(&dest_kind_to_cast, ptr_type);
        let val_ty = builder.func.dfg.value_type(value);
        if dest_cl_ty != val_ty {
            let is_unsigned = Self::is_unsigned_type_kind(&dest_kind_to_cast);
            value = Self::cast_value_with_sign(builder, value, val_ty, dest_cl_ty, is_unsigned)?;
        }
        Self::assign_to_place(builder, ctx, place, value, locals, type_ctx)?;

        if let Rvalue::Aggregate(AggregateKind::Set, ops) = rvalue {
            if ops.is_empty() {
                Self::apply_empty_set_init(builder, ctx, dest_ty, value, type_ctx)?;
            }
        }
        if let Rvalue::Aggregate(AggregateKind::Map, ops) = rvalue {
            if ops.is_empty() {
                Self::apply_empty_map_init(builder, ctx, dest_ty, value, type_ctx)?;
            }
        }
        Ok(())
    }

    /// Copies an inline vector element into an allocation of its own when it is
    /// bound to a local.
    ///
    /// A collection lays vector elements out inline at their std430 stride, so
    /// indexing one yields an address inside the collection's buffer. Binding
    /// that address would alias the collection — mutating the binding would edit
    /// the element, and releasing it would decrement a header the buffer never
    /// had. Copying restores the value semantics a vector is meant to have and
    /// gives the binding an allocation the reference counter can own.
    ///
    /// Any other destination (an element write, a field store) consumes the
    /// address directly, so `value` passes through untouched.
    fn copy_bound_inline_vec(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rvalue: &Rvalue,
        value: cranelift_codegen::ir::Value,
        type_ctx: &TypeCtx,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let Rvalue::Use(Operand::Copy(source) | Operand::Move(source)) = rvalue else {
            return Ok(value);
        };
        if !matches!(source.projection.last(), Some(PlaceElem::Index(_))) {
            return Ok(value);
        }
        let element_kind = Self::resolve_projected_type_kind(source, type_ctx);
        let Some((stride, dim, component)) =
            crate::codegen::cranelift::translator::inline_vec_element_layout(
                &element_kind,
                ptr_type,
            )
        else {
            return Ok(value);
        };

        let copy = Self::alloc_aggregate_payload(
            builder,
            ctx,
            ptr_type,
            ptr_type.bytes() as i32,
            stride as u32,
        )?;
        let component_bytes = component.bytes() as i32;
        for lane in 0..dim as i32 {
            let offset = lane * component_bytes;
            let read = builder
                .ins()
                .load(component, MemFlags::new(), value, offset);
            builder.ins().store(MemFlags::new(), read, copy, offset);
        }
        Ok(copy)
    }

    /// After an empty `Map<K,V>()` constructor: derive the key and value types
    /// from the destination annotation and register the key kind plus the drop
    /// and clone callbacks.
    ///
    /// A map literal takes all of this from its key and value operands, which an
    /// empty constructor does not have. Without the key kind the runtime hashes
    /// and compares a string key by its pointer, so two equal strings become two
    /// entries; without the drop callbacks the map never releases the references
    /// its callers donate.
    fn apply_empty_map_init(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        dest_ty: &crate::ast::types::Type,
        map_ptr: cranelift_codegen::ir::Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let Some((key_expr, value_expr)) = FunctionTranslator::map_key_value_exprs(&dest_ty.kind)
        else {
            return Ok(());
        };
        Self::register_map_key_callbacks(builder, ctx, key_expr, map_ptr, type_ctx)?;
        Self::register_map_value_callbacks_for_type(builder, ctx, value_expr, map_ptr, type_ctx)?;
        Ok(())
    }

    /// Registers the key kind and key drop callback for a map with managed keys.
    ///
    /// A string key also switches the map to content-based comparison; every
    /// other managed key keeps the default byte comparison and only needs the
    /// drop callback, so the runtime releases each key it discards.
    fn register_map_key_callbacks(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_expr: &Expression,
        map_ptr: cranelift_codegen::ir::Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ExpressionKind::Type(key_ty, _) = &key_expr.node else {
            return Ok(());
        };
        if matches!(key_ty.kind, TypeKind::String) {
            let key_kind_val = builder
                .ins()
                .iconst(ptr_type, FunctionTranslator::MANAGED_STRING_KEY_KIND);
            FunctionTranslator::call_rt_map_set_key_kind(builder, ctx, map_ptr, key_kind_val)?;
        }

        let Some(drop_fn_addr) = FunctionTranslator::key_decref_addr_for_kind(
            builder,
            ctx,
            &key_ty.kind,
            ptr_type,
            type_ctx,
        )?
        else {
            return Ok(());
        };
        FunctionTranslator::call_rt_map_set_key_drop_fn(builder, ctx, map_ptr, drop_fn_addr)?;
        Ok(())
    }

    /// Registers the value drop and clone callbacks for a managed value type.
    fn register_map_value_callbacks_for_type(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        value_expr: &Expression,
        map_ptr: cranelift_codegen::ir::Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ExpressionKind::Type(value_ty, _) = &value_expr.node else {
            return Ok(());
        };
        if FunctionTranslator::is_unresolved_generic_elem(&value_ty.kind, type_ctx.type_definitions)
        {
            return Ok(());
        }

        if let Some(drop_addr) = FunctionTranslator::elem_decref_addr_for_kind(
            builder,
            ctx,
            &value_ty.kind,
            ptr_type,
            type_ctx,
        )? {
            FunctionTranslator::call_rt_map_set_val_drop_fn(builder, ctx, map_ptr, drop_addr)?;
        }

        let shape = FunctionTranslator::classify_element_shape(&value_ty.kind);
        if let Some(clone_addr) = FunctionTranslator::elem_clone_addr_for_shape(
            builder,
            ctx,
            shape,
            type_ctx.type_definitions,
            ptr_type,
        )? {
            FunctionTranslator::call_rt_map_set_val_clone_fn(builder, ctx, map_ptr, clone_addr)?;
        }
        Ok(())
    }

    /// After an empty `Set<T>()` constructor: derive the element type from the
    /// destination annotation and register `elem_drop_fn` + `elem_clone_fn`.
    fn apply_empty_set_init(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        dest_ty: &crate::ast::types::Type,
        set_ptr: cranelift_codegen::ir::Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let Some(elem_expr) = FunctionTranslator::set_elem_expr(&dest_ty.kind) else {
            return Ok(());
        };
        let ExpressionKind::Type(elem_ty, _) = &elem_expr.node else {
            return Ok(());
        };
        FunctionTranslator::emit_set_drop_fn_for_elem_kind(
            builder,
            ctx,
            &elem_ty.kind,
            set_ptr,
            type_ctx,
        )?;
        FunctionTranslator::emit_set_clone_fn_for_elem_kind(
            builder,
            ctx,
            &elem_ty.kind,
            set_ptr,
            ptr_type,
            type_ctx.type_definitions,
        )?;
        Ok(())
    }
    /// Translate a terminator.
    pub(crate) fn translate_terminator(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        terminator: &Terminator,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        match &terminator.kind {
            TerminatorKind::Return => Self::translate_return(builder, body, locals, type_ctx),
            TerminatorKind::Goto { target } => {
                builder.ins().jump(blocks[target], &[]);
                Ok(())
            }
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => Self::translate_switch_int(
                builder, ctx, discr, targets, otherwise, locals, blocks, type_ctx,
            ),
            TerminatorKind::Call {
                func,
                args,
                out_args,
                arg_handles: _,
                destination,
                target,
            } => Self::translate_call(
                builder,
                ctx,
                func,
                args,
                out_args,
                destination,
                target.as_ref(),
                body,
                locals,
                blocks,
                type_ctx,
            ),
            TerminatorKind::Unreachable => {
                let trap_code = TrapCode::user(1).ok_or_else(|| {
                    CodegenError::Internal("Failed to create user trap code".to_string())
                })?;
                builder.ins().trap(trap_code);
                Ok(())
            }
            TerminatorKind::GpuLaunch {
                kernel,
                grid,
                block: grid_block,
                launch_args,
                scalar_args,
                uniform_bound_x,
                uniform_bound_y,
                uniform_bound_z,
                uniform_start_x,
                uniform_start_y,
                uniform_start_z,
                destination: _,
                target,
            } => {
                crate::codegen::cranelift::gpu_launch::translate(
                    builder,
                    ctx,
                    kernel,
                    grid,
                    grid_block,
                    launch_args,
                    scalar_args,
                    uniform_bound_x,
                    uniform_bound_y,
                    uniform_bound_z,
                    uniform_start_x,
                    uniform_start_y,
                    uniform_start_z,
                    locals,
                    type_ctx,
                )?;
                if let Some(t) = target {
                    builder.ins().jump(blocks[t], &[]);
                }
                Ok(())
            }
            TerminatorKind::VirtualCall {
                vtable_slot,
                args,
                out_args,
                destination,
                target,
            } => Self::translate_virtual_call(
                builder,
                ctx,
                *vtable_slot,
                args,
                out_args,
                destination,
                target.as_ref(),
                body,
                locals,
                blocks,
                type_ctx,
            ),
        }
    }

    /// Translate `TerminatorKind::Return`: write back scalar out params via
    /// caller stack slots, then return local(0) (or void).
    fn translate_return(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        for (&param_local, &ptr_var) in type_ctx.out_param_ptr_vars {
            let ptr = builder.use_var(ptr_var);
            if let Some(&val_var) = locals.get(&param_local) {
                let val = builder.use_var(val_var);
                builder.ins().store(MemFlags::new(), val, ptr, 0);
            }
        }
        if let Some(&var) = locals.get(&Local(0)) {
            let ret_ty = &body.local_decls[0].ty;
            if ret_ty.kind != TypeKind::Void {
                let value = builder.use_var(var);
                builder.ins().return_(&[value]);
            } else {
                builder.ins().return_(&[]);
            }
        } else {
            builder.ins().return_(&[]);
        }
        Ok(())
    }

    /// Translate `TerminatorKind::SwitchInt`. Lowers to a chain of `icmp` +
    /// `brif`, with a final unconditional branch to `otherwise` when no target
    /// matches.
    #[allow(clippy::too_many_arguments)]
    fn translate_switch_int(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        discr: &Operand,
        targets: &[(crate::mir::terminator::Discriminant, BasicBlock)],
        otherwise: &BasicBlock,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let disc_val = Self::translate_operand(builder, ctx, discr, locals, type_ctx, None)?;
        let disc_ty = builder.func.dfg.value_type(disc_val);

        if targets.is_empty() {
            builder.ins().jump(blocks[otherwise], &[]);
            return Ok(());
        }
        if targets.len() == 1 {
            let (value, target) = &targets[0];
            let cmp_val = builder.ins().iconst(disc_ty, value.value() as i64);
            let cond = builder.ins().icmp(IntCC::Equal, disc_val, cmp_val);
            builder
                .ins()
                .brif(cond, blocks[target], &[], blocks[otherwise], &[]);
            return Ok(());
        }

        let mut remaining_targets: Vec<_> = targets.iter().collect();
        let otherwise_block = blocks[otherwise];
        while let Some((value, target)) = remaining_targets.pop() {
            let target_block = blocks[target];
            let cmp_val = builder.ins().iconst(disc_ty, value.value() as i64);
            let cond = builder.ins().icmp(IntCC::Equal, disc_val, cmp_val);
            if remaining_targets.is_empty() {
                builder
                    .ins()
                    .brif(cond, target_block, &[], otherwise_block, &[]);
            } else {
                let next_check = builder.create_block();
                builder.ins().brif(cond, target_block, &[], next_check, &[]);
                builder.seal_block(next_check);
                builder.switch_to_block(next_check);
            }
        }
        Ok(())
    }

    /// Translate `TerminatorKind::Call`: direct named call or indirect closure
    /// call, with scalar-out writeback and optional collection-construction
    /// post-processing (`miri_rt_list_new`, `miri_rt_list_new_from_managed_array`).
    #[allow(clippy::too_many_arguments)]
    fn translate_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        func: &Operand,
        args: &[Operand],
        out_args: &[bool],
        destination: &Place,
        target: Option<&BasicBlock>,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let func_name = Self::direct_call_name(func);

        let (arg_values, sig, out_arg_slots) = Self::prepare_call_args(
            builder,
            ctx,
            func_name.as_deref(),
            args,
            out_args,
            locals,
            type_ctx,
        )?;

        let dest_ty = &body.local_decls[destination.local.0].ty;
        let cl_dest_ty = translate_type(dest_ty, ptr_type);
        let reinterpret_as = Self::element_word_result_type(func_name.as_deref(), cl_dest_ty);
        let mut sig = sig;
        if dest_ty.kind != TypeKind::Void {
            sig.returns.push(AbiParam::new(
                reinterpret_as.map_or(cl_dest_ty, |_| ptr_type),
            ));
        }

        if let Some(func_name) = func_name {
            Self::dispatch_named_call(
                builder,
                ctx,
                &func_name,
                args,
                arg_values,
                sig,
                dest_ty,
                destination,
                locals,
                type_ctx,
                ptr_type,
                reinterpret_as,
            )?;
        } else {
            Self::dispatch_indirect_call(
                builder,
                ctx,
                func,
                arg_values,
                sig,
                dest_ty,
                destination,
                locals,
                type_ctx,
                ptr_type,
            )?;
        }

        Self::writeback_out_arg_slots(builder, &out_arg_slots, locals, type_ctx, ptr_type);
        if let Some(t) = target {
            builder.ins().jump(blocks[t], &[]);
        }
        Ok(())
    }

    /// Returns the static function name when `func` is a `Constant(Identifier)`
    /// operand (direct named call); `None` for indirect/closure calls or any
    /// non-identifier constant.
    fn direct_call_name(func: &Operand) -> Option<String> {
        match func {
            Operand::Constant(c) => match &c.literal {
                Literal::Identifier(name) => Some(name.clone()),
                Literal::Integer(_)
                | Literal::Float(_)
                | Literal::String(_)
                | Literal::Boolean(_)
                | Literal::Regex(_)
                | Literal::None => None,
            },
            Operand::Copy(_) | Operand::Move(_) => None,
        }
    }

    /// Read each scalar `out`-param stack slot back into its caller-side
    /// `Local`. Pairs were recorded during `prepare_call_args`.
    fn writeback_out_arg_slots(
        builder: &mut FunctionBuilder,
        out_arg_slots: &[(cranelift_codegen::ir::Value, Local)],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) {
        for (addr, local) in out_arg_slots {
            if let Some(&var) = locals.get(local) {
                let local_ty = translate_type(type_ctx.local_types[local.0], ptr_type);
                let loaded = builder.ins().load(local_ty, MemFlags::new(), *addr, 0);
                builder.def_var(var, loaded);
            }
        }
    }

    /// Translate every call argument, applying ptr-width widening for runtime
    /// collection calls and copy-in/copy-out for scalar `out` params. Returns
    /// `(arg_values, signature-with-params-filled, out_arg_slots)` where
    /// `out_arg_slots` records the stack slot + caller local for each scalar
    /// out arg so the caller can read updated values back after the call.
    #[allow(clippy::too_many_arguments)]
    fn prepare_call_args(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        func_name: Option<&str>,
        args: &[Operand],
        out_args: &[bool],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<PreparedCallArgs, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let predeclared_sig = Self::lookup_predeclared_sig(ctx, func_name);

        let mut sig = Signature::new(builder.func.signature.call_conv);
        let mut arg_values = Vec::with_capacity(args.len());
        let mut out_arg_slots: Vec<(cranelift_codegen::ir::Value, Local)> = Vec::new();

        for (i, arg) in args.iter().enumerate() {
            let val = Self::translate_operand(builder, ctx, arg, locals, type_ctx, None)?;
            let scalar_out_local = Self::scalar_out_local_for_arg(out_args, i, arg, type_ctx);
            let val = if let Some(local) = scalar_out_local {
                let addr = Self::box_value_in_stack_slot(builder, val, ptr_type);
                out_arg_slots.push((addr, local));
                addr
            } else {
                let is_unsigned = Self::operand_is_unsigned(arg, type_ctx);
                Self::cast_arg_to_predeclared(
                    builder,
                    val,
                    i,
                    func_name,
                    predeclared_sig.as_ref(),
                    ptr_type,
                    is_unsigned,
                )?
            };
            arg_values.push(val);
            sig.params
                .push(AbiParam::new(builder.func.dfg.value_type(val)));
        }
        Ok((arg_values, sig, out_arg_slots))
    }

    /// Look up `func_name`'s pre-existing module declaration so later call
    /// args can be cast to the declared param types (avoids DFG-widened
    /// mismatches like passing an `I64`-widened `u8` where `I8` is expected).
    fn lookup_predeclared_sig(ctx: &ModuleCtx, func_name: Option<&str>) -> Option<Signature> {
        use cranelift_module::FuncOrDataId;
        let name = func_name?;
        if let Some(FuncOrDataId::Func(id)) = ctx.module.get_name(name) {
            Some(
                ctx.module
                    .declarations()
                    .get_function_decl(id)
                    .signature
                    .clone(),
            )
        } else {
            None
        }
    }

    /// If arg `i` is flagged as `out` and points to a scalar that needs a
    /// caller-provided stack slot, return the caller-side `Local` to write
    /// back to after the call; otherwise `None`.
    pub fn scalar_out_local_for_arg(
        out_args: &[bool],
        i: usize,
        arg: &Operand,
        type_ctx: &TypeCtx,
    ) -> Option<Local> {
        if !out_args.get(i).copied().unwrap_or(false) {
            return None;
        }
        match arg {
            Operand::Copy(p) | Operand::Move(p)
                if p.projection.is_empty()
                    && needs_out_pointer(&type_ctx.local_types[p.local.0].kind) =>
            {
                Some(p.local)
            }
            Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => None,
        }
    }

    /// Allocate a stack slot sized to `val`'s Cranelift type, store `val`
    /// into it, and return the slot's address as a ptr-typed value. Used to
    /// box scalar `out`-param values so the callee can write through them.
    fn box_value_in_stack_slot(
        builder: &mut FunctionBuilder,
        val: cranelift_codegen::ir::Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> cranelift_codegen::ir::Value {
        let cur_val_ty = builder.func.dfg.value_type(val);
        let slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            cur_val_ty.bytes(),
            cur_val_ty.bytes().trailing_zeros() as u8,
        ));
        let addr = builder.ins().stack_addr(ptr_type, slot, 0);
        builder.ins().store(MemFlags::new(), val, addr, 0);
        addr
    }

    /// Whether an argument to a runtime function carries an element value as
    /// opaque bytes, and so must keep its bit pattern rather than be converted
    /// numerically.
    fn is_opaque_value_param(func_name: Option<&str>, arg_index: usize) -> bool {
        func_name.is_some_and(|name| {
            crate::runtime_fns::element_value_positions(name).contains(&arg_index)
        })
    }

    /// Cast a call argument to match the pre-declared parameter type when one
    /// is known. For opaque-value-parameter arguments (e.g., items passed to
    /// LIST_PUSH), preserves the bit pattern via stack-slot reinterpretation
    /// rather than numeric conversion. For other arguments, uses `cast_value_with_sign`
    /// for proper numeric conversion.
    fn cast_arg_to_predeclared(
        builder: &mut FunctionBuilder,
        val: cranelift_codegen::ir::Value,
        i: usize,
        func_name: Option<&str>,
        predeclared_sig: Option<&Signature>,
        ptr_type: cranelift_codegen::ir::Type,
        is_unsigned: bool,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let Some(pre_sig) = predeclared_sig else {
            return Ok(val);
        };
        if i >= pre_sig.params.len() {
            return Ok(val);
        }
        let expected_ty = pre_sig.params[i].value_type;
        let actual_ty = builder.func.dfg.value_type(val);
        if actual_ty == expected_ty {
            return Ok(val);
        }

        if Self::is_opaque_value_param(func_name, i) {
            if actual_ty.bytes() == expected_ty.bytes() {
                Self::bitcast_via_stack(builder, val, actual_ty, expected_ty, ptr_type)
                    .map_err(|e| CodegenError::Internal(format!("Bitcast for arg {i} failed: {e}")))
            } else if actual_ty.bytes() < expected_ty.bytes() {
                if actual_ty.is_float() {
                    // A narrower-than-word float would have to be placed in the
                    // low bytes of the value word and read back at the
                    // collection's element stride. That stride is not yet
                    // resolved for sub-word floats, so the element reads back as
                    // zero. Refuse rather than store a value that cannot be
                    // recovered.
                    Err(CodegenError::Internal(format!(
                        "Call arg {i}: {actual_ty} element values are not supported in \
                         collections yet; the element stride for floats narrower than \
                         {expected_ty} is unresolved and the value would read back as zero"
                    )))
                } else {
                    Ok(builder.ins().uextend(expected_ty, val))
                }
            } else {
                Ok(builder.ins().ireduce(expected_ty, val))
            }
        } else {
            crate::codegen::cranelift::translator::FunctionTranslator::cast_value_with_sign(
                builder,
                val,
                actual_ty,
                expected_ty,
                is_unsigned,
            )
            .map_err(|e| {
                CodegenError::Internal(format!(
                    "Call arg {i}: cast {actual_ty} -> {expected_ty} failed: {e}"
                ))
            })
        }
    }

    /// Reinterpret a value's bit pattern by storing to a stack slot and loading
    /// with the target type. Both types must be the same byte size.
    fn bitcast_via_stack(
        builder: &mut FunctionBuilder,
        val: cranelift_codegen::ir::Value,
        from_ty: cranelift_codegen::ir::Type,
        to_ty: cranelift_codegen::ir::Type,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<cranelift_codegen::ir::Value, String> {
        if from_ty.bytes() != to_ty.bytes() {
            return Err(format!(
                "bitcast_via_stack: cannot cast between {} and {} (different sizes)",
                from_ty, to_ty
            ));
        }

        let byte_size = from_ty.bytes();
        let align_log = byte_size.trailing_zeros() as u8;
        let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, byte_size, align_log);
        let slot = builder.create_sized_stack_slot(slot_data);

        let addr = builder.ins().stack_addr(ptr_type, slot, 0);

        builder.ins().store(MemFlags::new(), val, addr, 0);
        let loaded = builder.ins().load(to_ty, MemFlags::new(), addr, 0);
        Ok(loaded)
    }

    /// Direct call to a named symbol: declare-import the callee, emit the
    /// Bind a call's result to the local the call names as its destination.
    fn define_call_destination(
        builder: &mut FunctionBuilder,
        locals: &HashMap<Local, Variable>,
        destination: &Place,
        result: cranelift_codegen::ir::Value,
    ) -> Result<(), CodegenError> {
        let dest_var = locals.get(&destination.local).ok_or_else(|| {
            CodegenError::Internal(format!(
                "Unknown call destination local: {:?}",
                destination.local
            ))
        })?;
        builder.def_var(*dest_var, result);
        Ok(())
    }

    /// The type a call's result must be reinterpreted into, when the call hands
    /// back element bytes in a value word rather than a value of the
    /// destination's type.
    ///
    /// Only a float destination needs this. Every other destination is already
    /// read out of the register the word arrives in, so the word is the value.
    fn element_word_result_type(
        func_name: Option<&str>,
        cl_dest_ty: cranelift_codegen::ir::Type,
    ) -> Option<cranelift_codegen::ir::Type> {
        let hands_back_bytes = func_name.is_some_and(crate::runtime_fns::returns_element_value);
        (hands_back_bytes && cl_dest_ty.is_float()).then_some(cl_dest_ty)
    }

    /// `call` instruction, store the result into the destination local, and
    /// run any runtime-specific post-call initialization (List drop-fn /
    /// clone-fn registration for `miri_rt_list_new*`).
    #[allow(clippy::too_many_arguments)]
    fn dispatch_named_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        func_name: &str,
        args: &[Operand],
        arg_values: Vec<cranelift_codegen::ir::Value>,
        sig: Signature,
        dest_ty: &crate::ast::types::Type,
        destination: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
        reinterpret_result_as: Option<cranelift_codegen::ir::Type>,
    ) -> Result<(), CodegenError> {
        let func_id = ctx
            .module
            .declare_function(func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.to_string(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &arg_values);

        let maybe_result = if dest_ty.kind != TypeKind::Void {
            let raw = builder.inst_results(call)[0];
            let result = match reinterpret_result_as {
                Some(dest_cl_ty) => {
                    let raw_ty = builder.func.dfg.value_type(raw);
                    Self::bitcast_via_stack(builder, raw, raw_ty, dest_cl_ty, ptr_type).map_err(
                        |e| {
                            CodegenError::Internal(format!(
                                "Reinterpreting {func_name}'s element word as {dest_cl_ty} failed: {e}"
                            ))
                        },
                    )?
                }
                None => raw,
            };
            Self::define_call_destination(builder, locals, destination, result)?;
            Some(result)
        } else {
            None
        };

        if func_name == rt::LIST_NEW_FROM_MANAGED_ARRAY {
            Self::apply_list_from_managed_overrides(builder, ctx, maybe_result, args, type_ctx)?;
        }
        if func_name == rt::LIST_NEW {
            Self::apply_list_new_init(builder, ctx, maybe_result, dest_ty, type_ctx, ptr_type)?;
        }
        Ok(())
    }

    /// Indirect call through a closure pointer. Loads fn_ptr from
    /// `payload[0]`, prepends env_ptr (= closure_ptr) to args and sig, and
    /// invokes via `call_indirect`.
    #[allow(clippy::too_many_arguments)]
    fn dispatch_indirect_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        func: &Operand,
        arg_values: Vec<cranelift_codegen::ir::Value>,
        sig: Signature,
        dest_ty: &crate::ast::types::Type,
        destination: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let closure_ptr = Self::translate_operand(builder, ctx, func, locals, type_ctx, None)?;
        let fn_ptr = builder
            .ins()
            .load(ptr_type, MemFlags::new(), closure_ptr, 0);

        let mut full_args = vec![closure_ptr];
        full_args.extend_from_slice(&arg_values);

        let mut full_sig = Signature::new(builder.func.signature.call_conv);
        full_sig.params.push(AbiParam::new(ptr_type)); // env_ptr
        full_sig.params.extend(sig.params);
        full_sig.returns.extend(sig.returns);

        let sig_ref = builder.import_signature(full_sig);
        let call = builder.ins().call_indirect(sig_ref, fn_ptr, &full_args);

        if dest_ty.kind != TypeKind::Void {
            let result = builder.inst_results(call)[0];
            Self::define_call_destination(builder, locals, destination, result)?;
        }
        Ok(())
    }

    /// After `miri_rt_list_new_from_managed_array`: the runtime preset is
    /// `elem_drop_fn = miri_rt_list_decref_element`. Override here for
    /// non-List managed element types (Array/Set/Map/UserClass) so the
    /// per-element decref dispatched on clear/remove_at matches the actual
    /// element kind. Also registers the clone helper for Cloneable user classes.
    fn apply_list_from_managed_overrides(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        maybe_result: Option<cranelift_codegen::ir::Value>,
        args: &[Operand],
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let (Some(list_ptr), Some(array_arg)) = (maybe_result, args.first()) else {
            return Ok(());
        };
        let array_kind = match array_arg {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                Some(&type_ctx.local_types[p.local.0].kind)
            }
            Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => None,
        };
        let Some(array_kind) = array_kind else {
            return Ok(());
        };
        let Some(elem_expr) = FunctionTranslator::collection_elem_expr(array_kind) else {
            return Ok(());
        };
        let ExpressionKind::Type(inner_ty, _) = &elem_expr.node else {
            return Ok(());
        };
        let shape = FunctionTranslator::classify_element_shape(&inner_ty.kind);
        let needs_decref_override = matches!(
            shape,
            ElementShape::Builtin(
                crate::ast::types::BuiltinCollectionKind::Array
                    | crate::ast::types::BuiltinCollectionKind::Set
                    | crate::ast::types::BuiltinCollectionKind::Map,
            ) | ElementShape::UserClass(_)
        );
        if needs_decref_override {
            // Route through `elem_decref_addr_for_kind` (not the shape-only
            // helper): a generic-class element (`Box<String>`) resolves to its
            // per-instantiation `__decref_Box__String` thunk so `clear`/`remove_at`
            // release the concrete managed field. The array-argument type — unlike
            // the element operand temps — preserves the concrete type arguments.
            if let Some(addr) = FunctionTranslator::elem_decref_addr_for_kind(
                builder,
                ctx,
                &inner_ty.kind,
                ptr_type,
                type_ctx,
            )? {
                FunctionTranslator::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)?;
            }
        }
        if let Some(addr) = FunctionTranslator::elem_clone_addr_for_shape(
            builder,
            ctx,
            shape,
            type_ctx.type_definitions,
            ptr_type,
        )? {
            FunctionTranslator::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// After `miri_rt_list_new` (empty `List<T>()` constructor): set
    /// `elem_drop_fn` and `elem_clone_fn` from the destination type's
    /// element annotation, since the constructor has no operands to inspect.
    fn apply_list_new_init(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        maybe_result: Option<cranelift_codegen::ir::Value>,
        dest_ty: &crate::ast::types::Type,
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let Some(list_ptr) = maybe_result else {
            return Ok(());
        };
        let Some(elem_expr) = FunctionTranslator::collection_elem_expr(&dest_ty.kind) else {
            return Ok(());
        };
        let ExpressionKind::Type(elem_ty, _) = &elem_expr.node else {
            return Ok(());
        };
        FunctionTranslator::emit_list_drop_fn_for_elem_kind(
            builder,
            ctx,
            &elem_ty.kind,
            list_ptr,
            type_ctx,
        )?;
        FunctionTranslator::emit_list_clone_fn_for_elem_kind(
            builder,
            ctx,
            &elem_ty.kind,
            list_ptr,
            ptr_type,
            type_ctx.type_definitions,
        )?;
        Ok(())
    }

    /// Translate `TerminatorKind::VirtualCall`: load fn-ptr from receiver's
    /// vtable slot and invoke via `call_indirect`. Scalar `out` params use the
    /// same copy-in/copy-out stack-slot ABI as direct `Call`.
    #[allow(clippy::too_many_arguments)]
    fn translate_virtual_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        vtable_slot: usize,
        args: &[Operand],
        out_args: &[bool],
        destination: &Place,
        target: Option<&BasicBlock>,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        debug_assert!(
            !args.is_empty(),
            "VirtualCall must have at least one arg (receiver)"
        );

        let (arg_values, mut sig, out_arg_slots) =
            Self::prepare_call_args(builder, ctx, None, args, out_args, locals, type_ctx)?;
        let dest_ty = &body.local_decls[destination.local.0].ty;
        if dest_ty.kind != TypeKind::Void {
            sig.returns
                .push(AbiParam::new(translate_type(dest_ty, ptr_type)));
        }

        let fn_ptr = Self::load_vtable_fn_ptr(builder, arg_values[0], vtable_slot, ptr_type);
        let sig_ref = builder.import_signature(sig);
        let call = builder.ins().call_indirect(sig_ref, fn_ptr, &arg_values);

        Self::store_vcall_result(builder, call, dest_ty, destination, locals)?;
        Self::writeback_out_arg_slots(builder, &out_arg_slots, locals, type_ctx, ptr_type);
        if let Some(t) = target {
            builder.ins().jump(blocks[t], &[]);
        }
        Ok(())
    }

    /// Load `vtable[slot]` from the receiver. Layout: receiver[0] = vtable ptr,
    /// then `vtable[slot * ptr_size]` holds the resolved function pointer.
    fn load_vtable_fn_ptr(
        builder: &mut FunctionBuilder,
        receiver_ptr: cranelift_codegen::ir::Value,
        vtable_slot: usize,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> cranelift_codegen::ir::Value {
        let vtable_ptr = builder
            .ins()
            .load(ptr_type, MemFlags::new(), receiver_ptr, 0);
        let slot_offset = (vtable_slot as i32) * ptr_type.bytes() as i32;
        builder
            .ins()
            .load(ptr_type, MemFlags::new(), vtable_ptr, slot_offset)
    }

    /// Write the (non-void) result of a virtual call back into the destination
    /// local. No-op for void return types.
    fn store_vcall_result(
        builder: &mut FunctionBuilder,
        call: cranelift_codegen::ir::Inst,
        dest_ty: &crate::ast::types::Type,
        destination: &Place,
        locals: &HashMap<Local, Variable>,
    ) -> Result<(), CodegenError> {
        if dest_ty.kind == TypeKind::Void {
            return Ok(());
        }
        let result = builder.inst_results(call)[0];
        let dest_var = locals.get(&destination.local).ok_or_else(|| {
            CodegenError::Internal(format!(
                "Unknown vcall destination local: {:?}",
                destination.local
            ))
        })?;
        builder.def_var(*dest_var, result);
        Ok(())
    }

    /// Resolve the `TypeKind` of a place after following its `Field` projections.
    ///
    /// For an unprojected local, returns the local's type kind directly.
    /// For a `Field(i)` projection on a `Custom` type, looks up the field type from
    /// `type_definitions` so that callers like `emit_type_drop` receive the correct
    /// kind (e.g. `List([int])`) instead of the container's kind (e.g. `Custom("Holder")`).
    /// For generic custom types (e.g. `Vec3<f32>`), substitutes the type arguments into
    /// the field type so that accessing a field returns the concrete type, not a generic.
    fn substitute_first_generic(
        field_type: &TypeKind,
        type_args: Option<&Vec<Expression>>,
        def_generics: Option<&Vec<crate::type_checker::context::GenericDefinition>>,
    ) -> TypeKind {
        if let (TypeKind::Generic(param_name, _, _), Some(args)) = (field_type, type_args) {
            let is_first_param = def_generics
                .and_then(|g| g.first())
                .map(|g| &g.name == param_name)
                .unwrap_or(false);

            if is_first_param && !args.is_empty() {
                if let ExpressionKind::Type(ty, _) = &args[0].node {
                    return ty.kind.clone();
                }
            }
        }
        field_type.clone()
    }

    pub(crate) fn resolve_projected_type_kind(place: &Place, type_ctx: &TypeCtx) -> TypeKind {
        let mut current = type_ctx.local_types[place.local.0].kind.clone();

        for proj in &place.projection {
            match proj {
                PlaceElem::Field(idx) => {
                    current = match &current {
                        TypeKind::Custom(name, type_args_opt) => {
                            use crate::type_checker::context::TypeDefinition;
                            match type_ctx.type_definitions.get(name.as_str()) {
                                Some(TypeDefinition::Struct(def)) => {
                                    let field_type = def
                                        .fields
                                        .get(*idx)
                                        .map(|(_, ty, _)| ty.kind.clone())
                                        .unwrap_or(TypeKind::Error);

                                    // Only apply substitution for compiler-known Vec types.
                                    if crate::ast::types::vec_type_dim(&current).is_some() {
                                        Self::substitute_first_generic(
                                            &field_type,
                                            type_args_opt.as_ref(),
                                            def.generics.as_ref(),
                                        )
                                    } else {
                                        field_type
                                    }
                                }
                                Some(TypeDefinition::Class(def)) => {
                                    let all_fields =
                                        crate::type_checker::context::collect_class_fields_all(
                                            def,
                                            type_ctx.type_definitions,
                                        );
                                    let field_type = all_fields
                                        .get(*idx)
                                        .map(|(_, fi)| fi.ty.kind.clone())
                                        .unwrap_or(TypeKind::Error);

                                    // Monomorphize a generic-parameter field to its
                                    // concrete type argument (scalar `T` at its real
                                    // width); concrete fields pass through unchanged.
                                    crate::type_checker::generics::substitute_generic_field_kind(
                                        &field_type,
                                        type_args_opt.as_deref(),
                                        def.generics.as_ref(),
                                    )
                                }
                                None
                                | Some(TypeDefinition::Enum(_))
                                | Some(TypeDefinition::Generic(_))
                                | Some(TypeDefinition::Alias(_))
                                | Some(TypeDefinition::Trait(_)) => TypeKind::Error,
                            }
                        }
                        // Closure env field: capture `idx` is looked up in the
                        // per-closure capture-type table stored in the TypeCtx.
                        TypeKind::Function(_) => type_ctx
                            .closure_capture_ast_types
                            .get(&place.local)
                            .and_then(|caps| caps.get(*idx))
                            .map(|ty| ty.kind.clone())
                            .unwrap_or(TypeKind::Error),
                        TypeKind::Int
                        | TypeKind::I8
                        | TypeKind::I16
                        | TypeKind::I32
                        | TypeKind::I64
                        | TypeKind::I128
                        | TypeKind::U8
                        | TypeKind::U16
                        | TypeKind::U32
                        | TypeKind::U64
                        | TypeKind::U128
                        | TypeKind::Float
                        | TypeKind::F16
                        | TypeKind::F32
                        | TypeKind::F64
                        | TypeKind::String
                        | TypeKind::Boolean
                        | TypeKind::Identifier
                        | TypeKind::RawPtr
                        | TypeKind::List(_)
                        | TypeKind::Array(_, _)
                        | TypeKind::Map(_, _)
                        | TypeKind::Set(_)
                        | TypeKind::Tuple(_)
                        | TypeKind::Result(_, _)
                        | TypeKind::Future(_)
                        | TypeKind::Generic(_, _, _)
                        | TypeKind::Meta(_)
                        | TypeKind::Option(_)
                        | TypeKind::Void
                        | TypeKind::Error
                        | TypeKind::Linear(_) => TypeKind::Error,
                    };
                }
                PlaceElem::Index(_) => {
                    current = Self::extract_collection_elem_type_kind(&current);
                }
                PlaceElem::Deref => break,
            }
        }

        current
    }

    /// Resolves the element type of a collection kind, returning an owned
    /// clone of the TypeKind. Uses the same logic as other resolvers (see
    /// predicates.rs) to ensure consistency across all element-type resolution.
    /// Element kind of `collection_kind`, or `TypeKind::Error` when it is not a
    /// collection or its element kind cannot be read.
    ///
    /// Delegates to the single element-type resolver rather than matching over
    /// collection kinds again: this walker and the address walker disagreeing
    /// about what an index projection yields is what silently corrupted indexed
    /// writes, so there is one home for that answer and this is a view onto it.
    /// `Error` is the fail-closed choice — it carries no width a store could be
    /// emitted against, where a plausible-looking kind would write the wrong
    /// number of bytes.
    fn extract_collection_elem_type_kind(collection_kind: &TypeKind) -> TypeKind {
        Self::resolve_collection_elem_type_kind_impl(collection_kind)
            .cloned()
            .unwrap_or(TypeKind::Error)
    }
}
