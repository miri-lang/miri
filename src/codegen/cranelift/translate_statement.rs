// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::codegen::cranelift::rc::ElementIdentitySetters;
use crate::codegen::cranelift::translator::{
    CallSite, ElementShape, FunctionTranslator, ModuleCtx, RuntimeChecks, TypeCtx,
};
use crate::codegen::cranelift::types::{translate_type, translate_type_kind};
use crate::error::CodegenError;
use crate::mir::symbol::Symbol;
use crate::mir::{
    arg_mode, AggregateKind, ArgMode, BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::{is_copy_on_write, rt};
use cranelift_codegen::ir::{
    condcodes::IntCC, AbiParam, Block, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, TrapCode,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

/// A list `miri_rt_list_new_from_managed_array` just built from an array
/// literal, with the element type it holds.
#[derive(Clone, Copy)]
struct ListLiteral<'t> {
    list_ptr: cranelift_codegen::ir::Value,
    elem_ty: &'t crate::ast::types::Type,
}

/// Output of `prepare_call_args`: per-arg Cranelift values, the partially-built
/// call signature (params filled, returns appended later by the caller), and
/// the list of scalar-out stack slots paired with their caller-side `Local`s
/// for post-call writeback.
type PreparedCallArgs = (
    Vec<cranelift_codegen::ir::Value>,
    Signature,
    Vec<(cranelift_codegen::ir::Value, Local)>,
);

/// Word offsets of the fields of a list's header (`MiriList`, laid out in C
/// order) that a push compiled inline reads and writes.
const LIST_DATA: i32 = 0;
const LIST_LEN: i32 = 1;
const LIST_CAPACITY: i32 = 2;
const LIST_ELEM_SIZE: i32 = 3;

/// A `miri_rt_list_push` call compiled inline: the list, the address of the
/// element's bytes, the number of them as passed, and the width the element
/// is loaded and stored at.
struct ListPush {
    list: cranelift_codegen::ir::Value,
    element: cranelift_codegen::ir::Value,
    payload: cranelift_codegen::ir::Value,
    width: cranelift_codegen::ir::Type,
}

impl ListPush {
    /// The push `args` make, when the element's byte count is a constant one
    /// load and one store can move: 1, 2, 4 or 8.
    fn of(builder: &FunctionBuilder, args: &[cranelift_codegen::ir::Value]) -> Option<Self> {
        use cranelift_codegen::ir::{types, InstructionData, ValueDef};
        let [list, element, payload] = args else {
            return None;
        };
        let ValueDef::Result(inst, _) = builder.func.dfg.value_def(*payload) else {
            return None;
        };
        let InstructionData::UnaryImm { imm, .. } = builder.func.dfg.insts[inst] else {
            return None;
        };
        let width = [types::I8, types::I16, types::I32, types::I64]
            .into_iter()
            .find(|width| i64::from(width.bytes()) == imm.bits())?;
        Some(Self {
            list: *list,
            element: *element,
            payload: *payload,
            width,
        })
    }
}

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
    /// `StorageDead` (scope exit) and closes the activation its declaration
    /// opened. The handle id is a compile-time constant on the local's decl; the
    /// runtime resolves it to the innermost live activation. A non-`gpu` local
    /// carries no handle and this is a no-op.
    ///
    /// Each release closes one activation, so a handle shared by several locals
    /// (a `gpu`-to-`gpu` move, a `gpu let` bound to a reduction's output) is
    /// released only by its first-declared owner — see
    /// [`Self::is_device_handle_owner`].
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
        if decl.device_handle_borrowed || !Self::is_device_handle_owner(body, place.local) {
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

    /// True when `local` is the first-declared non-borrowed local carrying its
    /// device handle. Later sharers are declared inside the owner's scope, so
    /// the owner's `StorageDead` runs on every path theirs does and closes the
    /// activation exactly once.
    fn is_device_handle_owner(body: &Body, local: Local) -> bool {
        let handle = body.local_decls[local.0].device_handle;
        !body.local_decls[..local.0]
            .iter()
            .any(|decl| !decl.device_handle_borrowed && decl.device_handle == handle)
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
        Self::emit_rc_stats_hook(builder, ctx, rt::RC_STATS_DUP)?;
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
        let place_kind = Self::resolve_projected_type_kind(place, type_ctx)?;
        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;
        Self::emit_decref_value(builder, ctx, &place_kind, ptr, type_ctx)
    }

    /// `StatementKind::Dealloc`: unconditional cleanup — the caller has
    /// already determined this value's RC chain is done. Skips null pointers
    /// and immortal objects (RC < 0).
    fn translate_dealloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;
        let place_kind_cow = Self::resolve_projected_type_kind(place, type_ctx)?;
        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        builder.switch_to_block(rc_block);
        Self::emit_release_check(builder, ctx, ptr)?;
        let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size));
        let rc = builder.ins().load(
            ptr_type,
            cranelift_codegen::ir::MemFlags::new(),
            header_ptr,
            0,
        );

        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dealloc_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dealloc_block, &[]);

        builder.switch_to_block(dealloc_block);
        Self::emit_type_drop(builder, ctx, &place_kind_cow, ptr, header_ptr, type_ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(rc_block);
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
            Self::resolve_projected_type_kind(place, type_ctx)?
        };
        Self::check_optional_flow(rvalue, &dest_kind_to_cast, type_ctx)?;

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

    /// Refuses an assignment that moves an optional's word into an optional laid
    /// out apart from it: a `Some` built, a value used or a value cast, where
    /// one side is its payload's own pointer and the other a box.
    ///
    /// The bits move unchanged, so the destination would open a box that is
    /// not there, or hand on a box as the payload. The MIR verifier refuses
    /// the same flow; this keeps the backend from compiling it when MIR
    /// reaches it unverified. The absent value, and an optional spelled at
    /// `void` before it met a slot, are null under every layout.
    fn check_optional_flow(
        rvalue: &Rvalue,
        dest_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if !matches!(dest_kind, TypeKind::Option(_)) {
            return Ok(());
        }
        let source_kind = match rvalue {
            Rvalue::Aggregate(AggregateKind::Option(built), _) => built.kind.clone(),
            Rvalue::Use(operand) => Self::operand_projected_kind(operand, type_ctx)?,
            Rvalue::Cast(operand, _) => Self::operand_projected_kind(operand, type_ctx)?,
            Rvalue::Aggregate(..)
            | Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => return Ok(()),
        };
        if !matches!(source_kind, TypeKind::Option(_))
            || Self::is_absent_value(rvalue)
            || Self::is_absent_placeholder(&source_kind)
            || Self::is_absent_placeholder(dest_kind)
        {
            return Ok(());
        }
        let source_is_pointer = Self::nullable_pointer_layout(&source_kind, type_ctx)?;
        if source_is_pointer == Self::nullable_pointer_layout(dest_kind, type_ctx)? {
            return Ok(());
        }
        Err(CodegenError::Internal(format!(
            "an assignment moves `{source_kind:?}` into `{dest_kind:?}`, which are laid out \
             apart (one is its payload's own pointer, the other a box)"
        )))
    }

    /// Whether `kind` is `void?`, the type an absent value is spelled at before
    /// it meets a slot: it holds nothing, so it agrees with every layout.
    fn is_absent_placeholder(kind: &TypeKind) -> bool {
        matches!(kind, TypeKind::Option(payload) if payload.kind == TypeKind::Void)
    }

    /// Whether `rvalue` writes the absent value, null under every layout.
    fn is_absent_value(rvalue: &Rvalue) -> bool {
        let operand = if let Rvalue::Use(operand) = rvalue {
            operand
        } else if let Rvalue::Cast(operand, _) = rvalue {
            operand.as_ref()
        } else {
            return false;
        };
        matches!(operand, Operand::Constant(constant) if matches!(constant.literal, Literal::None))
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
        let element_kind = Self::resolve_projected_type_kind(source, type_ctx)?;
        let Some((stride, dim, component)) =
            crate::codegen::cranelift::translator::inline_vec_element_layout(
                &element_kind,
                type_ctx,
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

    /// Registers how the map matches its keys and, for managed keys, the drop
    /// callback that releases each key the runtime discards.
    ///
    /// A string key is matched by content and a class key through its own
    /// `equals`, exactly as a set of that type matches its elements.
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
        FunctionTranslator::emit_element_identity(
            builder,
            ctx,
            &key_ty.kind,
            map_ptr,
            type_ctx,
            ElementIdentitySetters {
                set_kind: FunctionTranslator::call_rt_map_set_key_kind,
                set_equals_fn: FunctionTranslator::call_rt_map_set_key_equals_fn,
                set_hash_fn: FunctionTranslator::call_rt_map_set_key_hash_fn,
            },
        )?;

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
        if FunctionTranslator::is_unresolved_generic_elem(
            &value_ty.kind,
            type_ctx.facts.definitions(),
        ) {
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

        if let Some(clone_addr) = FunctionTranslator::elem_clone_addr_for_kind(
            builder,
            ctx,
            &value_ty.kind,
            type_ctx.facts,
            ptr_type,
        )? {
            FunctionTranslator::call_rt_map_set_val_clone_fn(builder, ctx, map_ptr, clone_addr)?;
        }
        Ok(())
    }

    /// After an empty `Set<T>()` constructor: derive the element type from the
    /// destination annotation and register how elements are matched, plus
    /// `elem_drop_fn` + `elem_clone_fn`.
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
        FunctionTranslator::emit_element_identity(
            builder,
            ctx,
            &elem_ty.kind,
            set_ptr,
            type_ctx,
            ElementIdentitySetters {
                set_kind: FunctionTranslator::call_rt_set_set_elem_kind,
                set_equals_fn: FunctionTranslator::call_rt_set_set_elem_equals_fn,
                set_hash_fn: FunctionTranslator::call_rt_set_set_elem_hash_fn,
            },
        )?;
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
            type_ctx.facts,
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
                arg_modes,
                arg_handles: _,
                destination,
                target,
            } => Self::translate_call(
                builder,
                ctx,
                func,
                args,
                arg_modes,
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
                arg_modes,
                destination,
                target,
            } => Self::translate_virtual_call(
                builder,
                ctx,
                *vtable_slot,
                args,
                arg_modes,
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
        arg_modes: &[ArgMode],
        destination: &Place,
        target: Option<&BasicBlock>,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let callee = Self::direct_callee(func)?;
        let func_name = callee.map(Symbol::link_name);

        let (arg_values, sig, out_arg_slots) = Self::prepare_call_args(
            builder,
            ctx,
            func_name.as_deref(),
            args,
            arg_modes,
            locals,
            type_ctx,
        )?;

        let dest_ty = &body.local_decls[destination.local.0].ty;
        let mut sig = sig;
        if dest_ty.kind != TypeKind::Void {
            sig.returns
                .push(AbiParam::new(translate_type(dest_ty, ptr_type)));
        }

        if let Some(callee) = callee {
            Self::dispatch_named_call(
                builder,
                ctx,
                callee,
                args,
                arg_values,
                sig,
                dest_ty,
                destination,
                locals,
                type_ctx,
                ptr_type,
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

    /// The function a direct call names; `None` for an indirect or closure
    /// call through a place. A constant callee names no function and is an
    /// internal error rather than an indirect call.
    fn direct_callee(func: &Operand) -> Result<Option<&Symbol>, CodegenError> {
        match func {
            Operand::Function(function) => Ok(Some(&function.symbol)),
            Operand::Copy(_) | Operand::Move(_) => Ok(None),
            Operand::Constant(constant) => Err(CodegenError::Internal(format!(
                "a call's callee is the constant {constant}, not a function"
            ))),
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
        arg_modes: &[ArgMode],
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
            let out_local = Self::out_local_for_arg(arg_modes, i, arg);
            let val = if let Some(local) = out_local {
                let addr = Self::box_value_in_stack_slot(builder, val, ptr_type);
                out_arg_slots.push((addr, local));
                addr
            } else {
                let is_unsigned = Self::operand_is_unsigned(arg, type_ctx)?;
                Self::cast_arg_to_predeclared(
                    builder,
                    val,
                    i,
                    predeclared_sig.as_ref(),
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

    /// If arg `i` is passed as [`ArgMode::Out`] and names a local, return that local:
    /// the caller passes the address of a slot holding its value and reads the
    /// slot back after the call. A scalar the callee changed is copied back,
    /// and a managed value it replaced takes the place of the one it released.
    pub fn out_local_for_arg(arg_modes: &[ArgMode], i: usize, arg: &Operand) -> Option<Local> {
        match arg_mode(arg_modes, i) {
            ArgMode::Out => {}
            ArgMode::Borrowed | ArgMode::Owned => return None,
        }
        arg.bare_local()
    }

    /// Allocate a stack slot sized to `val`'s Cranelift type, store `val`
    /// into it, and return the slot's address as a ptr-typed value. Used to
    /// box scalar `out`-param values so the callee can write through them.
    fn box_value_in_stack_slot(
        builder: &mut FunctionBuilder,
        val: cranelift_codegen::ir::Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> cranelift_codegen::ir::Value {
        let bytes = builder.func.dfg.value_type(val).bytes();
        let slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            bytes,
            bytes.trailing_zeros().min(4) as u8,
        ));
        let addr = builder.ins().stack_addr(ptr_type, slot, 0);
        builder.ins().store(MemFlags::new(), val, addr, 0);
        addr
    }

    /// Cast a call argument to match the pre-declared parameter type when one
    /// is known, converting it numerically with `cast_value_with_sign`.
    fn cast_arg_to_predeclared(
        builder: &mut FunctionBuilder,
        val: cranelift_codegen::ir::Value,
        i: usize,
        predeclared_sig: Option<&Signature>,
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

    /// Direct call to a named symbol: declare-import the callee, emit the
    /// `call` instruction, store the result into the destination local, and
    /// run any runtime-specific post-call initialization (List drop-fn /
    /// clone-fn registration for `miri_rt_list_new*`).
    #[allow(clippy::too_many_arguments)]
    fn dispatch_named_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        args: &[Operand],
        arg_values: Vec<cranelift_codegen::ir::Value>,
        sig: Signature,
        dest_ty: &crate::ast::types::Type,
        destination: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        if callee.runtime_name() == Some(rt::RC_IS_SHARED) {
            let shared = Self::emit_inline_is_shared(builder, ctx, callee, &arg_values, sig)?;
            return Self::define_call_destination(builder, locals, destination, shared);
        }
        if callee.runtime_name().is_some_and(is_copy_on_write) {
            let unique = Self::emit_inline_copy_on_write(builder, ctx, callee, &arg_values, &sig)?;
            return Self::define_call_destination(builder, locals, destination, unique);
        }
        if callee.runtime_name() == Some(rt::RC_STATS_COW_COPY) {
            return Self::emit_rc_stats_hook(builder, ctx, rt::RC_STATS_COW_COPY);
        }
        if callee.runtime_name() == Some(rt::LIST_PUSH) {
            if let Some(push) = ListPush::of(builder, &arg_values) {
                return Self::emit_inline_list_push(builder, ctx, callee, push, &sig);
            }
        }
        let func_name = callee.link_name();
        let func_id = ctx
            .module
            .declare_function(&func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &arg_values);

        let maybe_result = if dest_ty.kind != TypeKind::Void {
            let result = builder.inst_results(call)[0];
            Self::define_call_destination(builder, locals, destination, result)?;
            Some(result)
        } else {
            None
        };

        let runtime_name = callee.runtime_name();
        if runtime_name == Some(rt::LIST_NEW_FROM_MANAGED_ARRAY) {
            let elem_ty = Self::list_literal_element_type(dest_ty, args, type_ctx);
            if let (Some(list_ptr), Some(elem_ty)) = (maybe_result, elem_ty) {
                let literal = ListLiteral { list_ptr, elem_ty };
                Self::apply_list_from_managed_overrides(builder, ctx, &literal, type_ctx)?;
            }
        }
        if runtime_name == Some(rt::LIST_NEW) {
            Self::apply_list_new_init(builder, ctx, maybe_result, dest_ty, type_ctx, ptr_type)?;
        }
        Ok(())
    }

    /// Answers `miri_rt_rc_is_shared(ptr)` without the call while nothing is
    /// tracking the heap: a null pointer is not shared, and otherwise the count
    /// in the word before the payload is compared with 1, so an immortal
    /// value's negative count reads as shared, as in the runtime. In a build
    /// that keeps the runtime's checks, the runtime answers while the heap
    /// guard or another tracking mode is on, so the guard still checks that
    /// the block is live.
    fn emit_inline_is_shared(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        arg_values: &[cranelift_codegen::ir::Value],
        sig: Signature,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let [ptr] = arg_values else {
            return Err(CodegenError::Internal(format!(
                "{} takes one pointer, got {} arguments",
                rt::RC_IS_SHARED,
                arg_values.len()
            )));
        };
        let answer_type = sig
            .returns
            .first()
            .map(|param| param.value_type)
            .ok_or_else(|| {
                CodegenError::Internal(format!("{} returns nothing", rt::RC_IS_SHARED))
            })?;

        let check_block = builder.create_block();
        let join_block = builder.create_block();
        builder.append_block_param(join_block, answer_type);

        let not_shared = builder.ins().iconst(answer_type, 0);
        builder
            .ins()
            .brif(*ptr, check_block, &[], join_block, &[not_shared.into()]);

        builder.switch_to_block(check_block);
        builder.seal_block(check_block);
        let answer = match ctx.runtime_checks {
            RuntimeChecks::CompiledOut => Self::emit_count_is_shared(builder, *ptr, answer_type),
            RuntimeChecks::Observable => {
                Self::emit_observable_is_shared(builder, ctx, callee, *ptr, &sig, answer_type)?
            }
        };
        builder.ins().jump(join_block, &[answer.into()]);

        builder.seal_block(join_block);
        builder.switch_to_block(join_block);
        Ok(builder.block_params(join_block)[0])
    }

    /// Answers whether `ptr` is shared inline while the runtime's tracking state
    /// reads off, and through the runtime otherwise, so the heap guard still
    /// checks that the block is live.
    fn emit_observable_is_shared(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        ptr: cranelift_codegen::ir::Value,
        sig: &Signature,
        answer_type: cranelift_codegen::ir::Type,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let count_block = builder.create_block();
        let runtime_block = builder.create_block();
        let join_block = builder.create_block();
        builder.append_block_param(join_block, answer_type);

        let tracking_off = Self::emit_tracking_is_off(builder, ctx)?;
        builder
            .ins()
            .brif(tracking_off, count_block, &[], runtime_block, &[]);

        builder.switch_to_block(count_block);
        let shared = Self::emit_count_is_shared(builder, ptr, answer_type);
        builder.ins().jump(join_block, &[shared.into()]);

        builder.switch_to_block(runtime_block);
        let answer = Self::call_pointer_runtime(builder, ctx, callee, ptr, sig)?;
        builder.ins().jump(join_block, &[answer.into()]);

        builder.seal_block(count_block);
        builder.seal_block(runtime_block);
        builder.seal_block(join_block);
        builder.switch_to_block(join_block);
        Ok(builder.block_params(join_block)[0])
    }

    /// Calls a collection's copy-on-write entry point only when its answer is
    /// not already known: a null pointer and a value whose count is 1 come
    /// back unchanged, as the runtime would hand them back, without the call.
    /// Every other count — shared, or the negative count of an immortal value —
    /// takes the call, which hands back a copy. In a build that keeps the
    /// runtime's checks, the call is also taken whenever the runtime's tracking
    /// state does not read off, so the heap guard still validates the block
    /// before the write, and the call settles a state not yet read.
    fn emit_inline_copy_on_write(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        arg_values: &[cranelift_codegen::ir::Value],
        sig: &Signature,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let [ptr] = arg_values else {
            return Err(CodegenError::Internal(format!(
                "{} takes one pointer, got {} arguments",
                callee.link_name(),
                arg_values.len()
            )));
        };
        let ptr_type = builder.func.dfg.value_type(*ptr);
        let count_block = builder.create_block();
        let runtime_block = builder.create_block();
        let join_block = builder.create_block();
        builder.append_block_param(join_block, ptr_type);

        builder
            .ins()
            .brif(*ptr, count_block, &[], join_block, &[(*ptr).into()]);

        builder.switch_to_block(count_block);
        builder.seal_block(count_block);
        if ctx.runtime_checks == RuntimeChecks::Observable {
            let tracking_off = Self::emit_tracking_is_off(builder, ctx)?;
            let unobserved_block = builder.create_block();
            builder
                .ins()
                .brif(tracking_off, unobserved_block, &[], runtime_block, &[]);
            builder.switch_to_block(unobserved_block);
            builder.seal_block(unobserved_block);
        }
        let shared = Self::emit_count_is_shared(builder, *ptr, cranelift_codegen::ir::types::I8);
        builder
            .ins()
            .brif(shared, runtime_block, &[], join_block, &[(*ptr).into()]);

        builder.switch_to_block(runtime_block);
        builder.seal_block(runtime_block);
        let copy = Self::call_pointer_runtime(builder, ctx, callee, *ptr, sig)?;
        builder.ins().jump(join_block, &[copy.into()]);

        builder.seal_block(join_block);
        builder.switch_to_block(join_block);
        Ok(builder.block_params(join_block)[0])
    }

    /// Appends an element to a list without the call while the list has room
    /// for it: the element's bytes are stored into the next slot and the
    /// length is bumped. A null list, a full one, a slot wider than the
    /// element, and — in a build that keeps the runtime's checks — any push
    /// while the runtime is tracking take the call, which grows the buffer,
    /// zeroes a slot's spare bytes and lets the heap guard see the list.
    fn emit_inline_list_push(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        push: ListPush,
        sig: &Signature,
    ) -> Result<(), CodegenError> {
        let ptr_type = builder.func.dfg.value_type(push.list);
        let word = ptr_type.bytes() as i32;
        let header_block = builder.create_block();
        let store_block = builder.create_block();
        let runtime_block = builder.create_block();
        let join_block = builder.create_block();

        builder
            .ins()
            .brif(push.list, header_block, &[], runtime_block, &[]);
        builder.switch_to_block(header_block);
        builder.seal_block(header_block);
        if ctx.runtime_checks == RuntimeChecks::Observable {
            let tracking_off = Self::emit_tracking_is_off(builder, ctx)?;
            let unobserved_block = builder.create_block();
            builder
                .ins()
                .brif(tracking_off, unobserved_block, &[], runtime_block, &[]);
            builder.switch_to_block(unobserved_block);
            builder.seal_block(unobserved_block);
        }
        let header = MemFlags::trusted();
        let len = builder
            .ins()
            .load(ptr_type, header, push.list, LIST_LEN * word);
        let capacity = builder
            .ins()
            .load(ptr_type, header, push.list, LIST_CAPACITY * word);
        let slot = builder
            .ins()
            .load(ptr_type, header, push.list, LIST_ELEM_SIZE * word);
        let has_room = builder.ins().icmp(IntCC::UnsignedLessThan, len, capacity);
        let fills_slot = builder
            .ins()
            .icmp_imm(IntCC::Equal, slot, push.width.bytes() as i64);
        let fast = builder.ins().band(has_room, fills_slot);
        builder
            .ins()
            .brif(fast, store_block, &[], runtime_block, &[]);

        builder.switch_to_block(store_block);
        builder.seal_block(store_block);
        let data = builder
            .ins()
            .load(ptr_type, header, push.list, LIST_DATA * word);
        let offset = builder.ins().imul_imm(len, push.width.bytes() as i64);
        let dest = builder.ins().iadd(data, offset);
        let element = builder
            .ins()
            .load(push.width, MemFlags::new(), push.element, 0);
        builder.ins().store(MemFlags::trusted(), element, dest, 0);
        let grown = builder.ins().iadd_imm(len, 1);
        builder
            .ins()
            .store(header, grown, push.list, LIST_LEN * word);
        builder.ins().jump(join_block, &[]);

        builder.switch_to_block(runtime_block);
        builder.seal_block(runtime_block);
        let func_name = callee.link_name();
        let func_id = ctx
            .module
            .declare_function(&func_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder
            .ins()
            .call(local_func, &[push.list, push.element, push.payload]);
        builder.ins().jump(join_block, &[]);

        builder.seal_block(join_block);
        builder.switch_to_block(join_block);
        Ok(())
    }

    /// Whether the count in the word before the payload at `ptr` is other than
    /// 1, as a value of `answer_type`.
    fn emit_count_is_shared(
        builder: &mut FunctionBuilder,
        ptr: cranelift_codegen::ir::Value,
        answer_type: cranelift_codegen::ir::Type,
    ) -> cranelift_codegen::ir::Value {
        let ptr_type = builder.func.dfg.value_type(ptr);
        let header = ptr_type.bytes() as i32;
        let count = builder
            .ins()
            .load(ptr_type, MemFlags::trusted(), ptr, -header);
        let shared = builder.ins().icmp_imm(IntCC::NotEqual, count, 1);
        if answer_type == cranelift_codegen::ir::types::I8 {
            shared
        } else {
            builder.ins().uextend(answer_type, shared)
        }
    }

    /// Calls the one-pointer runtime entry point `callee` on `ptr` and returns
    /// its answer.
    fn call_pointer_runtime(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        ptr: cranelift_codegen::ir::Value,
        sig: &Signature,
    ) -> Result<cranelift_codegen::ir::Value, CodegenError> {
        let func_name = callee.link_name();
        let func_id = ctx
            .module
            .declare_function(&func_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[ptr]);
        Ok(builder.inst_results(call)[0])
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
    /// `elem_drop_fn = miri_rt_list_decref_element`, which reads each element as
    /// a list. Every other element kind — a string, a collection, a class, a
    /// structural value — gets the decref its own kind names, so the per-element
    /// release dispatched on clear/remove/remove_at matches the element. Also
    /// registers the clone helper for Cloneable user classes and the order.
    fn apply_list_from_managed_overrides(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        literal: &ListLiteral,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ListLiteral { list_ptr, elem_ty } = *literal;
        Self::override_list_literal_drop_fn(builder, ctx, literal, type_ctx)?;
        let ptr_type = type_ctx.ptr_type;
        if let Some(addr) = FunctionTranslator::elem_clone_addr_for_kind(
            builder,
            ctx,
            &elem_ty.kind,
            type_ctx.facts,
            ptr_type,
        )? {
            FunctionTranslator::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        FunctionTranslator::emit_element_order(
            builder,
            ctx,
            &elem_ty.kind,
            list_ptr,
            type_ctx,
            FunctionTranslator::LIST_ORDER_SETTERS,
        )?;
        Ok(())
    }

    /// The element type of a list built from an array literal.
    ///
    /// The list the call produces carries the element type the program asked
    /// for, so it comes first. The literal's own type is inferred from its
    /// elements and can leave an argument none of them fixes open — a
    /// lone `Result.Err(s)` is a `Result<T, String>` — which would name the
    /// shared release helper, blind to the payload. It answers only when the
    /// destination names no element.
    fn list_literal_element_type<'t>(
        dest_ty: &'t crate::ast::types::Type,
        args: &[Operand],
        type_ctx: &'t TypeCtx,
    ) -> Option<&'t crate::ast::types::Type> {
        let element_of = |kind: &'t TypeKind| match FunctionTranslator::collection_elem_expr(kind) {
            Some(Expression {
                node: ExpressionKind::Type(inner, _),
                ..
            }) => Some(&**inner),
            Some(_) | None => None,
        };
        element_of(&dest_ty.kind).or_else(|| match args.first()? {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                element_of(&type_ctx.local_types[p.local.0].kind)
            }
            Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => {
                None
            }
        })
    }

    /// Replace the runtime's list-shaped `elem_drop_fn` preset for any element
    /// that is not itself a list. Where the element's kind names no release
    /// helper, the drop function is cleared rather than left reading the
    /// element as a list: an element released through the wrong layout is
    /// freed at the wrong size, where one that is not released only leaks.
    fn override_list_literal_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        literal: &ListLiteral,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ListLiteral { list_ptr, elem_ty } = *literal;
        let shape = FunctionTranslator::classify_element_shape(&elem_ty.kind);
        let is_list = match shape {
            ElementShape::Builtin(BuiltinCollectionKind::List) => true,
            ElementShape::Builtin(
                BuiltinCollectionKind::Array
                | BuiltinCollectionKind::Set
                | BuiltinCollectionKind::Map,
            )
            | ElementShape::String
            | ElementShape::UserClass(_)
            | ElementShape::Other => false,
        };
        if is_list {
            return Ok(());
        }
        let ptr_type = type_ctx.ptr_type;
        // Route through `elem_decref_addr_for_kind` (not the shape-only helper):
        // a generic-class element (`Box<String>`) resolves to its
        // per-instantiation `miri.Box$String.$decref` thunk, and a structural
        // element to the thunk made for its structure.
        let addr = match FunctionTranslator::elem_decref_addr_for_kind(
            builder,
            ctx,
            &elem_ty.kind,
            ptr_type,
            type_ctx,
        )? {
            Some(addr) => addr,
            // Every managed element resolves a release address; a list literal
            // of one that does not would free nothing, or free it at the wrong
            // layout, so it is refused rather than compiled.
            None => {
                return Err(CodegenError::Internal(format!(
                    "a list literal of `{}` elements has no release function for them",
                    elem_ty
                )))
            }
        };
        FunctionTranslator::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)
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
            type_ctx.facts,
        )?;
        FunctionTranslator::emit_element_order(
            builder,
            ctx,
            &elem_ty.kind,
            list_ptr,
            type_ctx,
            FunctionTranslator::LIST_ORDER_SETTERS,
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
        arg_modes: &[ArgMode],
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
            Self::prepare_call_args(builder, ctx, None, args, arg_modes, locals, type_ctx)?;
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

    /// Resolve the `TypeKind` of a place after following its projections.
    ///
    /// For an unprojected local, returns the local's type kind directly. A
    /// `Field(i)` projection on a struct or class reads the field's type at
    /// the instance's own type arguments through [`TypeFacts::field_types`],
    /// so callers like `emit_type_drop` receive the kind the field actually
    /// stores (e.g. `List([int])`) instead of the container's kind.
    ///
    /// [`TypeFacts::field_types`]: crate::mir::type_facts::TypeFacts::field_types
    pub(crate) fn resolve_projected_type_kind(
        place: &Place,
        type_ctx: &TypeCtx,
    ) -> Result<TypeKind, CodegenError> {
        let mut walk = ProjectedKind::new(place, type_ctx);
        for proj in &place.projection {
            walk.step(proj, type_ctx)?;
        }
        Ok(walk.kind)
    }

    /// The kind of field `idx` of a value of kind `container`.
    ///
    /// A declared type and a closure answer from their fields (a closure's
    /// captures are held in the per-closure table rather than in any
    /// declaration), and a tuple from the element type it spells. A following
    /// projection is laid out by this answer: read as the tuple instead, a
    /// struct field of a tuple element (`p.0.n`) would load at the tuple's own
    /// field offset. An optional's one field is its payload, so a release or a
    /// following projection through it sees the payload's own type — the
    /// difference between opening a box and reading a nullable pointer as
    /// itself. The rest — a result's payload — have no declared field to read,
    /// and resolve to `Error`: their reader loads at the width of the binding
    /// it fills.
    ///
    /// A closure local missing from the capture table, or a capture index past
    /// the ones recorded, is a lowering invariant broken and is reported.
    fn projected_field_kind(
        container: &TypeKind,
        idx: usize,
        place: &Place,
        type_ctx: &TypeCtx,
    ) -> Result<TypeKind, CodegenError> {
        match container {
            TypeKind::Custom(name, type_args) => {
                Self::declared_field_kind(name, type_args.as_deref(), idx, type_ctx)
            }
            TypeKind::Function(_) => type_ctx
                .closure_capture_ast_types
                .get(&place.local)
                .and_then(|caps| caps.get(idx))
                .map(|ty| ty.kind.clone())
                .ok_or_else(|| {
                    CodegenError::Internal(format!(
                        "closure local {:?} records no captured value at field {idx}",
                        place.local
                    ))
                }),
            TypeKind::Tuple(elements) => Ok(elements
                .get(idx)
                .and_then(|element| {
                    let ExpressionKind::Type(ty, _) = &element.node else {
                        return None;
                    };
                    Some(ty.kind.clone())
                })
                .unwrap_or(TypeKind::Error)),
            TypeKind::Option(payload) => Ok(payload.kind.clone()),
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
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::OneOf(_)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => Ok(TypeKind::Error),
        }
    }

    /// The kind of field `idx` of the declared type `name`, read at the type
    /// arguments the value carries.
    ///
    /// A struct and a class both read their field at the instance's arguments:
    /// `value T` at a scalar stores at that scalar's width, and `items List<T>`
    /// at `String` holds managed elements, so a store through the field
    /// releases what it replaces. An inherited field is read at the type the
    /// `extends` chain binds it to.
    ///
    /// An enum reaches its payload by matching, not by field index, and its
    /// reader loads at the width of the binding it fills; a name declaring no
    /// fields at all answers the same way, with `Error`. A field index past the
    /// fields a struct or class stores means MIR and the definition disagree
    /// about the type, and is reported rather than read at a guessed width.
    fn declared_field_kind(
        name: &str,
        type_args: Option<&[Expression]>,
        idx: usize,
        type_ctx: &TypeCtx,
    ) -> Result<TypeKind, CodegenError> {
        let Some(fields) = type_ctx.facts.field_types(name, type_args) else {
            return Ok(TypeKind::Error);
        };
        fields.get(idx).map(|ty| ty.kind.clone()).ok_or_else(|| {
            CodegenError::Internal(format!(
                "field {idx} is projected from `{name}`, which stores {} fields",
                fields.len()
            ))
        })
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

/// The kind [`FunctionTranslator::resolve_projected_type_kind`] gives each
/// prefix of a place, advanced one projection at a time. A walk over a place's
/// projections that needs the type at every depth steps this along with it,
/// rather than re-resolving each prefix from the root.
pub(crate) struct ProjectedKind<'p> {
    place: &'p Place,
    pub(crate) kind: TypeKind,
    /// A `Deref` ends the resolution: what lies behind it keeps the kind of
    /// the pointer's own place.
    stopped: bool,
}

impl<'p> ProjectedKind<'p> {
    pub(crate) fn new(place: &'p Place, type_ctx: &TypeCtx) -> Self {
        ProjectedKind {
            place,
            kind: type_ctx.local_types[place.local.0].kind.clone(),
            stopped: false,
        }
    }

    /// Applies one more projection of the place and returns the kind reached.
    pub(crate) fn step(
        &mut self,
        proj: &PlaceElem,
        type_ctx: &TypeCtx,
    ) -> Result<&TypeKind, CodegenError> {
        if !self.stopped {
            match proj {
                PlaceElem::Field(idx) => {
                    self.kind = FunctionTranslator::projected_field_kind(
                        &self.kind, *idx, self.place, type_ctx,
                    )?;
                }
                PlaceElem::Index(_) => {
                    self.kind = FunctionTranslator::extract_collection_elem_type_kind(&self.kind);
                }
                PlaceElem::Deref => self.stopped = true,
            }
        }
        Ok(&self.kind)
    }
}
