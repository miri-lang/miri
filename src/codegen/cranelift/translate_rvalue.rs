// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::layout::field_layout;
use crate::codegen::cranelift::translator::{CallSite, FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::error::CodegenError;
use crate::mir::{
    AggregateKind, BinOp, Constant, Local, MathIntrinsic, Operand, Place, Rvalue, UnOp,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::class_needs_vtable;
use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types as cl_types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, TrapCode, Value,
};
use cranelift_codegen::ir::{AbiParam, Signature};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

/// User trap code raised on heap allocation failure (malloc returning null).
///
/// Built via `TrapCode::unwrap_user`, a `const fn` whose invalid-code branch
/// fails to compile rather than panic at run time — so this constant is
/// evaluated entirely at compile time.
const OOM_TRAP_CODE: TrapCode = TrapCode::unwrap_user(2);

/// Per-container runtime setter callbacks used by `register_elem_drop_clone`.
#[derive(Clone, Copy)]
struct ElementCallbackSetters {
    set_drop: fn(&mut FunctionBuilder, &mut ModuleCtx, Value, Value) -> Result<(), CodegenError>,
    set_clone: fn(&mut FunctionBuilder, &mut ModuleCtx, Value, Value) -> Result<(), CodegenError>,
}

impl<'a> FunctionTranslator<'a> {
    fn emit_libm_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        func_name: &str,
        ty: cl_types::Type,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        let mut sig = Signature::new(builder.func.signature.call_conv);
        for _ in arg_values {
            sig.params.push(AbiParam::new(ty));
        }
        sig.returns.push(AbiParam::new(ty));

        let func_id = ctx
            .module
            .declare_function(func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, arg_values);
        Ok(builder.inst_results(call)[0])
    }

    /// Check that an offset fits into i32 for memory operations. Returns error if the
    /// offset exceeds i32::MAX, preventing silent truncation of large aggregate layouts
    /// (>2 GiB).
    fn store_offset_i32(offset: i64) -> Result<i32, CodegenError> {
        i32::try_from(offset).map_err(|_| {
            CodegenError::Internal("aggregate layout exceeds 2 GiB addressable space".to_string())
        })
    }

    /// Translate a MIR rvalue to a Cranelift value.
    ///
    /// When `expected_ty` is Some, it is passed to operand translation for
    /// Rvalue::Use to resolve enum/Option field load widths.
    pub(crate) fn translate_rvalue(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected_ty: Option<&crate::ast::types::Type>,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;

        match rvalue {
            Rvalue::Use(operand) => {
                Self::translate_operand(builder, ctx, operand, locals, type_ctx, expected_ty)
            }

            Rvalue::BinaryOp(op, lhs, rhs) => {
                Self::translate_binary_op(builder, ctx, *op, lhs, rhs, locals, type_ctx)
            }

            Rvalue::UnaryOp(op, operand) => {
                let val = Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)?;
                Self::translate_unop(builder, *op, val)
            }

            Rvalue::Ref(place) => {
                let value = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;
                let val_ty = builder.func.dfg.value_type(value);
                let size = val_ty.bytes();
                let align = size; // Simplification for scalars
                let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, size, align as u8);
                let slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(ptr_type, slot, 0);
                builder.ins().store(MemFlags::new(), value, addr, 0);
                Ok(addr)
            }

            Rvalue::Aggregate(kind, operands) => Self::translate_aggregate(
                builder,
                ctx,
                kind,
                operands,
                locals,
                type_ctx,
                expected_ty,
            ),

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx, None)?;
                let dest_ty = translate_type(ty, ptr_type);
                let src_ty = builder.func.dfg.value_type(value);
                // Signedness follows the integer side of the cast: the source for
                // int→float and int→int (so a `u32` zero-extends and converts as
                // unsigned), the destination for float→int. Keying off the
                // destination alone wrongly treats a `u32` with its top bit set as
                // a negative value when converting to float.
                // The projected kind, because a field or element read has the
                // type of what it reaches: judging by the base local reports a
                // class or a list, which is not an integer at all, and the cast
                // then falls back to signed and widens `200` in a `u8` to `-56`.
                let src_kind = Self::operand_projected_kind(operand, type_ctx);
                let is_unsigned = if Self::is_integer_kind(&src_kind) {
                    Self::is_unsigned_type_kind(&src_kind)
                } else {
                    Self::is_unsigned_type_kind(&ty.kind)
                };

                Self::cast_value_with_sign(builder, value, src_ty, dest_ty, is_unsigned)
            }

            Rvalue::Len(place) => Self::translate_len(builder, ctx, place, locals, type_ctx),

            Rvalue::GpuIntrinsic(_intrinsic) => Err(CodegenError::Internal(
                "GPU intrinsics not supported in CPU backend".to_string(),
            )),

            Rvalue::MathIntrinsic(intrinsic, args) => {
                Self::translate_math_intrinsic(builder, ctx, *intrinsic, args, locals, type_ctx)
            }

            Rvalue::Phi(_) => Err(CodegenError::Internal(
                "Phi nodes must be eliminated before codegen. Run SSA destruction pass."
                    .to_string(),
            )),

            Rvalue::AtomicOp { .. } => Err(CodegenError::Internal(
                "Atomic operations are GPU-only and not supported in CPU backend".to_string(),
            )),
        }
    }

    /// Translate an `Rvalue::BinaryOp` to a Cranelift value, taking the
    /// structural-equality path for tuples / structs and falling back to
    /// `translate_binop` for primitive ops.
    fn translate_binary_op(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        // Structural equality: compare field-by-field instead of pointer
        // comparison for tuples and structs. Only takes the structural path
        // when the operand denotes a *whole* aggregate. A `Copy(t.0)` is a
        // primitive field load even though `operand_type_kind` reports the
        // base local's tuple type — must not treat its value as a tuple pointer.
        if matches!(op, BinOp::Eq | BinOp::Ne)
            && Self::operand_has_no_projection(lhs)
            && Self::operand_has_no_projection(rhs)
        {
            if let Some(result) =
                Self::try_structural_equality(builder, ctx, lhs, rhs, locals, type_ctx)?
            {
                return if op == BinOp::Ne {
                    let one = builder.ins().iconst(cranelift_codegen::ir::types::I8, 1);
                    Ok(builder.ins().bxor(result, one))
                } else {
                    Ok(result)
                };
            }
        }

        let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx, None)?;
        let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx, None)?;
        let is_unsigned =
            Self::operand_is_unsigned(lhs, type_ctx) || Self::operand_is_unsigned(rhs, type_ctx);
        Self::translate_binop(builder, ctx, op, lhs_val, rhs_val, is_unsigned)
    }

    /// Returns the field-wise equality result for `lhs == rhs` when both
    /// operands are whole tuples or whole structs; returns `Ok(None)`
    /// otherwise so the caller falls back to primitive comparison.
    fn try_structural_equality(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        lhs: &Operand,
        rhs: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        let lhs_kind = Self::operand_type_kind(lhs, type_ctx);
        match lhs_kind {
            TypeKind::Tuple(element_exprs) => {
                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx, None)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx, None)?;
                Ok(Some(Self::translate_tuple_equality(
                    builder,
                    ctx,
                    lhs_val,
                    rhs_val,
                    element_exprs,
                    type_ctx,
                )?))
            }
            TypeKind::Custom(name, _) => {
                let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
                    type_ctx.type_definitions.get(name)
                else {
                    return Ok(None);
                };
                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx, None)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx, None)?;
                Ok(Some(Self::translate_struct_equality(
                    builder, lhs_val, rhs_val, lhs_kind, def, type_ctx,
                )?))
            }
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
            | TypeKind::Function(_)
            | TypeKind::Generic(_, _, _)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => Ok(None),
        }
    }

    /// Translate an `Rvalue::Aggregate` to a Cranelift value.
    /// `expected_ty` is the destination type at the assignment site, used to resolve
    /// Option<T> inner types and generic type parameters for payload field coercion.
    pub(crate) fn translate_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &AggregateKind,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected_ty: Option<&crate::ast::types::Type>,
    ) -> Result<Value, CodegenError> {
        // Handle closure allocation separately.
        if let AggregateKind::Closure(lambda_name, fn_type) = kind {
            return Self::translate_closure_aggregate(
                builder,
                ctx,
                lambda_name,
                fn_type,
                operands,
                locals,
                type_ctx,
            );
        }

        let is_collection = matches!(
            kind,
            AggregateKind::Array | AggregateKind::List | AggregateKind::Map | AggregateKind::Set
        );

        if is_collection {
            return Self::build_collection_aggregate(
                builder, ctx, kind, operands, locals, type_ctx,
            );
        }
        Self::build_struct_like_aggregate(
            builder,
            ctx,
            kind,
            operands,
            locals,
            type_ctx,
            expected_ty,
        )
    }

    /// Build a heap-allocated `Array`, `List`, `Map`, or `Set` aggregate from `operands`.
    fn build_collection_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &AggregateKind,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        // Translate all element operands
        let translated: Vec<Value> = operands
            .iter()
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx, None))
            .collect::<Result<_, _>>()?;

        // Determine element size from the first operand (all are homogeneous).
        // Inline vector elements occupy their std430 stride even though the
        // operand value is a pointer to the source aggregate; pointer-sized and
        // scalar elements use the operand's Cranelift width.
        let first_elem_kind: Option<&TypeKind> = operands.first().and_then(|op| match op {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                Some(&type_ctx.local_types[p.local.0].kind)
            }
            Operand::Constant(c) => Some(&c.ty.kind),
            Operand::Copy(_) | Operand::Move(_) => None,
        });
        let inline_stride = first_elem_kind.and_then(|k| {
            crate::codegen::cranelift::translator::inline_vec_element_layout(k, ptr_type)
                .map(|(stride, _, _)| stride)
        });
        let elem_size = match (inline_stride, translated.is_empty()) {
            (Some(stride), _) => stride,
            (None, true) => ptr_size as i64,
            (None, false) => builder.func.dfg.value_type(translated[0]).bytes() as i64,
        };
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);

        match kind {
            AggregateKind::Array => Self::build_array_aggregate(
                builder, ctx, operands, &translated, elem_size, elem_size_val, type_ctx,
            ),
            AggregateKind::List => Self::build_list_aggregate(
                builder, ctx, operands, translated, elem_size_val, type_ctx,
            ),
            AggregateKind::Map => Self::build_map_aggregate(
                builder, ctx, operands, translated, type_ctx,
            ),
            AggregateKind::Set => Self::build_set_aggregate(
                builder, ctx, operands, translated, elem_size_val, type_ctx,
            ),
            AggregateKind::Tuple
            | AggregateKind::Struct(_)
            | AggregateKind::Class(_)
            | AggregateKind::FormattedString
            | AggregateKind::Enum(_, _)
            | AggregateKind::Option
            | AggregateKind::Closure(_, _) => Err(CodegenError::Internal(format!(
                "internal codegen error: non-collection AggregateKind {:?} reached collection branch",
                kind
            ))),
        }
    }

    /// Build a heap-allocated `Array` aggregate populated from `translated`.
    fn build_array_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        translated: &[Value],
        elem_size: i64,
        elem_size_val: Value,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let count_val = builder.ins().iconst(ptr_type, operands.len() as i64);
        let array_ptr = Self::call_rt_array_new(builder, ctx, count_val, elem_size_val)?;

        // Only inspect non-projected operands: `first_operand_kind` returns the
        // LOCAL's declared type, which is wrong for field projections (e.g.
        // `child.value` where `child: Tree` has type Tree, not the field type
        // `int`). Skipping projected operands is safe — it leaves elem_drop_fn
        // null for those arrays, which merely preserves the pre-existing
        // behaviour for that case.
        let first_op_direct_kind: Option<&TypeKind> = operands.first().and_then(|op| match op {
            Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                Some(&type_ctx.local_types[p.local.0].kind)
            }
            Operand::Constant(c) => Some(&c.ty.kind),
            Operand::Copy(_) | Operand::Move(_) => None,
        });
        // Inline vector elements are copied component-by-component from the
        // source aggregate (`val` is its address); pointer-sized elements store
        // the operand value directly.
        let inline = first_op_direct_kind.and_then(|k| {
            crate::codegen::cranelift::translator::inline_vec_element_layout(k, ptr_type)
        });

        if !translated.is_empty() {
            // Read data pointer from MiriArray.data (offset 0)
            let data_ptr = builder.ins().load(ptr_type, MemFlags::new(), array_ptr, 0);
            for (i, val) in translated.iter().enumerate() {
                let offset = (i as i64) * elem_size;
                match inline {
                    Some((_, dim, comp_ty)) => {
                        let comp_bytes = comp_ty.bytes() as i64;
                        for k in 0..dim as i64 {
                            let comp_offset = Self::store_offset_i32(k * comp_bytes)?;
                            let comp =
                                builder
                                    .ins()
                                    .load(comp_ty, MemFlags::new(), *val, comp_offset);
                            let store_offset = Self::store_offset_i32(offset + k * comp_bytes)?;
                            builder
                                .ins()
                                .store(MemFlags::new(), comp, data_ptr, store_offset);
                        }
                    }
                    None => {
                        let store_offset = Self::store_offset_i32(offset)?;
                        builder
                            .ins()
                            .store(MemFlags::new(), *val, data_ptr, store_offset);
                    }
                }
            }
        }
        if let Some(elem_kind) = first_op_direct_kind {
            Self::register_elem_drop_clone(
                builder,
                ctx,
                elem_kind,
                array_ptr,
                ptr_type,
                type_ctx,
                ElementCallbackSetters {
                    set_drop: Self::call_rt_array_set_elem_drop_fn,
                    set_clone: Self::call_rt_array_set_elem_clone_fn,
                },
            )?;
        }
        Ok(array_ptr)
    }

    /// Build a heap-allocated `List` aggregate populated from `translated`.
    fn build_list_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        translated: Vec<Value>,
        elem_size_val: Value,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let list_ptr = Self::call_rt_list_new(builder, ctx, elem_size_val)?;

        for val in translated {
            // Widen or narrow to ptr_type for the FFI call
            let val_ty = builder.func.dfg.value_type(val);
            let widened = if val_ty.bytes() < ptr_type.bytes() {
                builder.ins().sextend(ptr_type, val)
            } else if val_ty.bytes() > ptr_type.bytes() {
                builder.ins().ireduce(ptr_type, val)
            } else {
                val
            };
            Self::call_rt_list_push(builder, ctx, list_ptr, widened)?;
        }

        if let Some(first_op) = operands.first() {
            if let Some(elem_kind) = Self::first_operand_kind(first_op, type_ctx) {
                Self::register_elem_drop_clone(
                    builder,
                    ctx,
                    elem_kind,
                    list_ptr,
                    ptr_type,
                    type_ctx,
                    ElementCallbackSetters {
                        set_drop: Self::call_rt_list_set_elem_drop_fn,
                        set_clone: Self::call_rt_list_set_elem_clone_fn,
                    },
                )?;
            }
        }
        Ok(list_ptr)
    }

    /// Build a heap-allocated `Map` aggregate populated from `translated`.
    fn build_map_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        translated: Vec<Value>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let (key_size, value_size, key_kind) =
            Self::map_aggregate_descriptor(builder, &translated, operands, type_ctx, ptr_type);

        let key_size_val = builder.ins().iconst(ptr_type, key_size);
        let value_size_val = builder.ins().iconst(ptr_type, value_size);
        let key_kind_val = builder.ins().iconst(ptr_type, key_kind);

        let map_ptr =
            Self::call_rt_map_new(builder, ctx, key_size_val, value_size_val, key_kind_val)?;

        Self::register_map_value_callbacks(builder, ctx, operands, map_ptr, ptr_type, type_ctx)?;
        Self::register_map_key_drop(builder, ctx, operands, map_ptr, ptr_type, type_ctx)?;

        for chunk in translated.chunks(2) {
            if chunk.len() == 2 {
                let key_val = Self::widen_to_ptr(builder, chunk[0], ptr_type);
                let val_val = Self::widen_to_ptr(builder, chunk[1], ptr_type);
                Self::call_rt_map_set(builder, ctx, map_ptr, key_val, val_val)?;
            }
        }
        Ok(map_ptr)
    }

    /// Returns `(key_size, value_size, key_kind)` for the upcoming map. `key_kind`
    /// is 1 when the first key is a `TypeKind::String` (so the runtime knows to
    /// DecRef string keys), 0 otherwise. Sizes fall back to pointer-size when the
    /// literal has no concrete entries to measure.
    fn map_aggregate_descriptor(
        builder: &FunctionBuilder,
        translated: &[Value],
        operands: &[Operand],
        type_ctx: &TypeCtx,
        ptr_type: cl_types::Type,
    ) -> (i64, i64, i64) {
        let ptr_size = ptr_type.bytes() as i64;
        let (key_size, value_size) = if translated.len() >= 2 {
            (
                builder.func.dfg.value_type(translated[0]).bytes() as i64,
                builder.func.dfg.value_type(translated[1]).bytes() as i64,
            )
        } else {
            (ptr_size, ptr_size)
        };
        let key_kind = match operands.first() {
            Some(op)
                if matches!(
                    Self::first_operand_kind(op, type_ctx),
                    Some(TypeKind::String)
                ) =>
            {
                1
            }
            _ => 0,
        };
        (key_size, value_size, key_kind)
    }

    /// Registers `key_drop_fn` for a map literal whose keys are managed, so the
    /// runtime releases each key when the map drops it. The key type is read
    /// from the first key operand, which a literal always carries.
    fn register_map_key_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        map_ptr: Value,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let Some(key_kind) = operands
            .first()
            .and_then(|op| Self::first_operand_kind(op, type_ctx))
        else {
            return Ok(());
        };
        let Some(drop_fn_addr) =
            Self::key_decref_addr_for_kind(builder, ctx, key_kind, ptr_type, type_ctx)?
        else {
            return Ok(());
        };
        Self::call_rt_map_set_key_drop_fn(builder, ctx, map_ptr, drop_fn_addr)
    }

    /// Registers `elem_drop_fn` / `elem_clone_fn` callbacks for the value side
    /// of a map, when the value type tells us which managed decref/clone helper
    /// to wire up.
    fn register_map_value_callbacks(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        map_ptr: Value,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if operands.len() < 2 {
            return Ok(());
        }
        let Some(val_kind) = Self::first_operand_kind(&operands[1], type_ctx) else {
            return Ok(());
        };
        Self::register_elem_drop_clone(
            builder,
            ctx,
            val_kind,
            map_ptr,
            ptr_type,
            type_ctx,
            ElementCallbackSetters {
                set_drop: Self::call_rt_map_set_val_drop_fn,
                set_clone: Self::call_rt_map_set_val_clone_fn,
            },
        )
    }

    /// Build a heap-allocated `Set` aggregate populated from `translated`.
    fn build_set_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        translated: Vec<Value>,
        elem_size_val: Value,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let set_ptr = Self::call_rt_set_new(builder, ctx, elem_size_val)?;

        for val in translated {
            let widened = Self::widen_to_ptr(builder, val, ptr_type);
            Self::call_rt_set_add(builder, ctx, set_ptr, widened)?;
        }

        if let Some(first_op) = operands.first() {
            if let Some(elem_kind) = Self::first_operand_kind(first_op, type_ctx) {
                Self::register_elem_drop_clone(
                    builder,
                    ctx,
                    elem_kind,
                    set_ptr,
                    ptr_type,
                    type_ctx,
                    ElementCallbackSetters {
                        set_drop: Self::call_rt_set_set_elem_drop_fn,
                        set_clone: Self::call_rt_set_set_elem_clone_fn,
                    },
                )?;
            }
        }
        Ok(set_ptr)
    }

    /// Register decref + clone runtime callbacks for an element kind onto a container.
    ///
    /// `setters` are the container-specific runtime setter callbacks (e.g.
    /// `call_rt_array_set_elem_drop_fn` / `call_rt_array_set_elem_clone_fn`).
    fn register_elem_drop_clone(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        container_ptr: Value,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
        setters: ElementCallbackSetters,
    ) -> Result<(), CodegenError> {
        if let Some(addr) =
            Self::elem_decref_addr_for_kind(builder, ctx, elem_kind, ptr_type, type_ctx)?
        {
            (setters.set_drop)(builder, ctx, container_ptr, addr)?;
        }
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_clone_addr_for_shape(
            builder,
            ctx,
            shape,
            type_ctx.type_definitions,
            ptr_type,
        )? {
            (setters.set_clone)(builder, ctx, container_ptr, addr)?;
        }
        Ok(())
    }

    /// Build a non-collection aggregate (Tuple / Struct / Class / Enum / Option /
    /// FormattedString). Lays out fields, heap-allocates with `[malloc_ptr][RC][payload]`
    /// header, and writes operand values into payload slots.
    fn build_struct_like_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &AggregateKind,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected_ty: Option<&crate::ast::types::Type>,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        let vtable_class_name = Self::class_vtable_name(kind, type_ctx);
        let needs_vtable_alloc = vtable_class_name.is_some();
        if operands.is_empty() && !needs_vtable_alloc {
            return Ok(builder.ins().iconst(ptr_type, 0));
        }

        let is_tuple = matches!(kind, AggregateKind::Tuple);
        let needs_pointer_layout = matches!(
            kind,
            AggregateKind::Struct(_)
                | AggregateKind::Class(_)
                | AggregateKind::Enum(_, _)
                | AggregateKind::Tuple
                | AggregateKind::Option
        );
        if operands.len() == 1 && !needs_pointer_layout {
            return Self::translate_operand(builder, ctx, &operands[0], locals, type_ctx, None);
        }

        let translated: Vec<Value> = operands
            .iter()
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx, None))
            .collect::<Result<_, _>>()?;

        let tuple_header = if is_tuple { ptr_size as u32 } else { 0 };
        let vtable_header_size = if needs_vtable_alloc {
            ptr_size as u32
        } else {
            0
        };
        let (field_offsets, total_size) = Self::compute_aggregate_layout(
            builder,
            &translated,
            tuple_header + vtable_header_size,
            is_tuple,
            matches!(kind, AggregateKind::Enum(_, _) | AggregateKind::Option),
            ptr_size as u32,
        )?;

        let payload_ptr =
            Self::alloc_aggregate_payload(builder, ctx, ptr_type, ptr_size, total_size)?;
        if is_tuple {
            let count = builder.ins().iconst(ptr_type, translated.len() as i64);
            builder.ins().store(MemFlags::new(), count, payload_ptr, 0);
        }
        if let Some(class_name) = vtable_class_name {
            Self::store_vtable_pointer(builder, ctx, &class_name, payload_ptr, ptr_type)?;
        }

        // Resolve declared field types for payload coercion.
        let declared_field_types =
            Self::resolve_declared_field_types_for_aggregate(kind, type_ctx, expected_ty);

        for (i, mut val) in translated.into_iter().enumerate() {
            if let Some(ref decl_types) = declared_field_types {
                let payload_field_idx = if matches!(kind, AggregateKind::Option) {
                    i
                } else if i > 0 {
                    i - 1
                } else {
                    i
                };
                if (i > 0 || matches!(kind, AggregateKind::Option))
                    && payload_field_idx < decl_types.len()
                {
                    if let Some(Some(decl_ty)) = decl_types.get(payload_field_idx) {
                        val = Self::coerce_value_to_declared_type(builder, val, decl_ty, ptr_type)?;
                    }
                }
            }
            let store_offset = Self::store_offset_i32(field_offsets[i] as i64)?;
            builder
                .ins()
                .store(MemFlags::new(), val, payload_ptr, store_offset);
        }
        Ok(payload_ptr)
    }

    /// Returns the class name when `kind` is `AggregateKind::Class(ty)` and
    /// the class participates in virtual dispatch (i.e. needs a vtable slot at
    /// `payload[0]`); otherwise returns `None`.
    fn class_vtable_name(kind: &AggregateKind, type_ctx: &TypeCtx) -> Option<String> {
        let AggregateKind::Class(ty) = kind else {
            return None;
        };
        let TypeKind::Custom(class_name, _) = &ty.kind else {
            return None;
        };
        if class_needs_vtable(class_name, type_ctx.type_definitions) {
            Some(class_name.clone())
        } else {
            None
        }
    }

    /// Resolve the declared payload types for an aggregate kind, one entry per
    /// payload field.
    ///
    /// An entry is `None` when the payload has no statically known width — a
    /// generic enum constructed inside a generic function body, where the type
    /// parameter is not bound at this site. Those fields keep the value's own
    /// width, matching the pointer-sized slot the match arm will read them back
    /// at. Returns `None` for aggregate kinds that carry no declared payload
    /// types at all.
    fn resolve_declared_field_types_for_aggregate(
        kind: &AggregateKind,
        type_ctx: &TypeCtx,
        expected_ty: Option<&crate::ast::types::Type>,
    ) -> Option<Vec<Option<crate::ast::types::Type>>> {
        match kind {
            AggregateKind::Enum(enum_name, variant_name) => {
                let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
                    type_ctx.type_definitions.get(enum_name.as_ref())
                else {
                    return None;
                };
                let declared = enum_def.variants.get(variant_name.as_ref())?;
                // A generic enum spells its payloads as type parameters; bind
                // them to the arguments of the type this aggregate is assigned
                // to, so the store width matches what the match arm loads.
                let type_args = expected_ty.and_then(|ty| Self::enum_instantiation_args(&ty.kind));
                Some(
                    declared
                        .iter()
                        .map(|ty| {
                            let kind = crate::type_checker::generics::substitute_generic_field_kind(
                                &ty.kind,
                                type_args.as_deref(),
                                enum_def.generics.as_ref(),
                            );
                            // A payload with no bound type argument has no known
                            // width, and a reference-counted one is
                            // pointer-sized whatever it is named. Both keep the
                            // value's own representation, matching the width
                            // the match arm reads them back at.
                            if crate::type_checker::generics::is_generic_parameter_kind(
                                &kind,
                                enum_def.generics.as_ref(),
                            ) || crate::codegen::cranelift::translator::is_field_managed(&kind)
                            {
                                None
                            } else {
                                Some(crate::ast::types::Type::new(kind, ty.span))
                            }
                        })
                        .collect(),
                )
            }
            AggregateKind::Option => expected_ty.and_then(|ty| {
                if let TypeKind::Option(inner) = &ty.kind {
                    Some(vec![Some((**inner).clone())])
                } else {
                    None
                }
            }),
            AggregateKind::Tuple
            | AggregateKind::Struct(_)
            | AggregateKind::Class(_)
            | AggregateKind::FormattedString
            | AggregateKind::Closure(_, _)
            | AggregateKind::Array
            | AggregateKind::List
            | AggregateKind::Map
            | AggregateKind::Set => None,
        }
    }

    /// The type arguments of an enum instantiation, in the order the enum
    /// declares its parameters. The type checker normalizes `Result<T, E>` to
    /// `TypeKind::Custom`, but the dedicated `TypeKind::Result` spelling also
    /// reaches codegen, so both forms are recognized.
    fn enum_instantiation_args(kind: &TypeKind) -> Option<Vec<crate::ast::Expression>> {
        if let TypeKind::Custom(_, Some(args)) = kind {
            Some(args.clone())
        } else if let TypeKind::Result(ok_ty, err_ty) = kind {
            Some(vec![(**ok_ty).clone(), (**err_ty).clone()])
        } else {
            None
        }
    }

    /// Coerce a value to its declared type, handling width mismatches for floats and ints.
    /// If the declared type is wider than ptr_type, skip coercion to prevent store overflow:
    /// enum payload slots are ptr-sized, so widening would write past the slot boundary.
    fn coerce_value_to_declared_type(
        builder: &mut FunctionBuilder,
        value: Value,
        declared_ty: &crate::ast::types::Type,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        let decl_cl_ty =
            crate::codegen::cranelift::types::translate_type_kind(&declared_ty.kind, ptr_type);
        let val_cl_ty = builder.func.dfg.value_type(value);
        let ptr_bytes = ptr_type.bytes();
        if decl_cl_ty != val_cl_ty && decl_cl_ty.bytes() <= ptr_bytes {
            let is_unsigned = Self::is_unsigned_type_kind(&declared_ty.kind);
            Self::cast_value_with_sign(builder, value, val_cl_ty, decl_cl_ty, is_unsigned)
        } else {
            Ok(value)
        }
    }

    /// Compute per-field offsets and total payload size for a struct-like aggregate.
    /// Uses u64 internally to prevent silent wraparound, then checks final size fits in u32.
    fn compute_aggregate_layout(
        builder: &FunctionBuilder,
        translated: &[Value],
        header_size: u32,
        is_tuple: bool,
        is_enum: bool,
        ptr_size: u32,
    ) -> Result<(Vec<u32>, u32), CodegenError> {
        let mut current_offset: u64 = header_size as u64;
        let mut field_offsets = Vec::with_capacity(translated.len());
        let mut max_align: u32 = if is_tuple { ptr_size } else { 1 };

        for &val in translated {
            let ty = builder.func.dfg.value_type(val);
            let align = if is_enum { ptr_size } else { ty.bytes() };
            max_align = max_align.max(align);

            let align_u64 = align as u64;
            current_offset = (current_offset + align_u64 - 1) & !(align_u64 - 1);
            let offset_u32 = u32::try_from(current_offset).map_err(|_| {
                CodegenError::Internal(
                    "aggregate layout exceeds 2 GiB addressable space".to_string(),
                )
            })?;
            field_offsets.push(offset_u32);
            current_offset += if is_enum {
                ptr_size as u64
            } else {
                ty.bytes() as u64
            };
        }
        let total_size_u64 = (current_offset + max_align as u64 - 1) & !(max_align as u64 - 1);
        let total_size = u32::try_from(total_size_u64).map_err(|_| {
            CodegenError::Internal("aggregate layout exceeds 2 GiB addressable space".to_string())
        })?;
        Ok((field_offsets, total_size))
    }

    /// Heap-allocate `[malloc_ptr][RC][payload]` for an aggregate, traps on OOM,
    /// and returns the payload pointer (past the 2-slot header). RC is initialized
    /// to 1.
    pub(crate) fn alloc_aggregate_payload(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cl_types::Type,
        ptr_size: i32,
        payload_size: u32,
    ) -> Result<Value, CodegenError> {
        let header_size = 2 * ptr_size as u32;
        let alloc_size = builder
            .ins()
            .iconst(ptr_type, (payload_size + header_size) as i64);
        let raw_ptr = Self::call_libc_malloc(builder, ctx, alloc_size)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, raw_ptr, null);
        builder.ins().trapnz(is_null, OOM_TRAP_CODE);

        // Store real malloc pointer at offset 0
        builder.ins().store(MemFlags::new(), raw_ptr, raw_ptr, 0);
        // Store RC = 1 at offset ptr_size
        let one = builder.ins().iconst(ptr_type, 1);
        builder.ins().store(MemFlags::new(), one, raw_ptr, ptr_size);

        Ok(builder.ins().iadd_imm(raw_ptr, header_size as i64))
    }

    /// Store the `__vtable_{class_name}` pointer at offset 0 of `payload_ptr`.
    fn store_vtable_pointer(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        class_name: &str,
        payload_ptr: Value,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        use cranelift_module::Module;
        let mut vtable_sym = String::with_capacity(9 + class_name.len());
        vtable_sym.push_str("__vtable_");
        vtable_sym.push_str(class_name);
        let vtable_data_id = ctx
            .module
            .declare_data(&vtable_sym, cranelift_module::Linkage::Import, false, false)
            .map_err(|e| CodegenError::declare_function(vtable_sym.clone(), e.to_string()))?;
        let gv = ctx
            .module
            .declare_data_in_func(vtable_data_id, builder.func);
        let vtable_ptr = builder.ins().global_value(ptr_type, gv);
        builder
            .ins()
            .store(MemFlags::new(), vtable_ptr, payload_ptr, 0);
        Ok(())
    }

    /// Translate an `Rvalue::Len` to a Cranelift value.
    pub(crate) fn translate_len(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let ty = type_ctx.local_types[place.local.0];

        let is_tuple_type = ty.kind.is_tuple();

        let len_offset = if Self::is_collection_type(&ty.kind) {
            // MiriArray.elem_count, MiriList.len, MiriSet.len at offset ptr_size.
            // MiriMap.len at offset 3*ptr_size (after states, keys, values).
            Some(if Self::is_map_type(&ty.kind) {
                ptr_size * 3
            } else {
                ptr_size
            })
        } else if matches!(&ty.kind, TypeKind::String) {
            Some(ptr_size)
        } else if is_tuple_type {
            Some(0)
        } else {
            None
        };

        let Some(offset) = len_offset else {
            return Ok(builder.ins().iconst(ptr_type, 0));
        };

        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx, None)?;

        // Handle null pointer (empty/uninitialized)
        let is_null = builder.ins().icmp_imm(IntCC::Equal, ptr, 0);
        let null_bb = builder.create_block();
        let load_bb = builder.create_block();
        let merge_bb = builder.create_block();
        let len_var = builder.declare_var(ptr_type);

        builder.ins().brif(is_null, null_bb, &[], load_bb, &[]);

        builder.switch_to_block(null_bb);
        let zero = builder.ins().iconst(ptr_type, 0);
        builder.def_var(len_var, zero);
        builder.ins().jump(merge_bb, &[]);
        builder.seal_block(null_bb);

        builder.switch_to_block(load_bb);
        let len = builder.ins().load(ptr_type, MemFlags::new(), ptr, offset);
        builder.def_var(len_var, len);
        builder.ins().jump(merge_bb, &[]);
        builder.seal_block(load_bb);

        builder.switch_to_block(merge_bb);
        builder.seal_block(merge_bb);

        Ok(builder.use_var(len_var))
    }

    /// Translate an `Rvalue::MathIntrinsic` to a Cranelift value.
    pub(crate) fn translate_math_intrinsic(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        intrinsic: MathIntrinsic,
        args: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let mut arg_values = Vec::with_capacity(args.len());
        for arg in args {
            arg_values.push(Self::translate_operand(
                builder, ctx, arg, locals, type_ctx, None,
            )?);
        }
        if arg_values.is_empty() {
            return Err(CodegenError::Internal(format!(
                "Math intrinsic {} expects at least one argument",
                intrinsic
            )));
        }
        // Every per-intrinsic emitter assumes all float operands share the
        // result width `ty` (taken from the first operand). Nested-intrinsic
        // inference can hand us a mix of widths — e.g. `mix(mix(..), mix(..), t)`
        // widens the endpoints to f64 while the interpolant `t` stays f32 — which
        // would emit a type-mismatched `fsub`/`fmul` and fail Cranelift
        // verification. When any operand is f64, compute at f64 (never silently
        // drop precision) and promote the narrower operands to match.
        let ty = if arg_values
            .iter()
            .any(|v| builder.func.dfg.value_type(*v) == cl_types::F64)
        {
            cl_types::F64
        } else {
            builder.func.dfg.value_type(arg_values[0])
        };
        if ty.is_float() {
            for value in arg_values.iter_mut() {
                let value_ty = builder.func.dfg.value_type(*value);
                if value_ty.is_float() && value_ty != ty {
                    *value = builder.ins().fpromote(ty, *value);
                }
            }
        }
        let is_f32 = ty == cl_types::F32;

        match intrinsic {
            MathIntrinsic::Abs => Ok(Self::emit_math_abs(builder, ty, arg_values[0])),
            MathIntrinsic::Sqrt => Self::emit_math_sqrt(builder, ty, arg_values[0]),
            MathIntrinsic::Ceil => Ok(Self::emit_math_unary_int_passthrough(
                builder,
                ty,
                arg_values[0],
                |b, v| b.ins().ceil(v),
            )),
            MathIntrinsic::Floor => Ok(Self::emit_math_unary_int_passthrough(
                builder,
                ty,
                arg_values[0],
                |b, v| b.ins().floor(v),
            )),
            MathIntrinsic::Round => Ok(Self::emit_math_unary_int_passthrough(
                builder,
                ty,
                arg_values[0],
                |b, v| b.ins().nearest(v),
            )),
            MathIntrinsic::Min => {
                Self::emit_math_min_max(builder, ctx, ty, is_f32, &arg_values, true)
            }
            MathIntrinsic::Max => {
                Self::emit_math_min_max(builder, ctx, ty, is_f32, &arg_values, false)
            }
            MathIntrinsic::Sin
            | MathIntrinsic::Cos
            | MathIntrinsic::Tan
            | MathIntrinsic::Ln
            | MathIntrinsic::Exp
            | MathIntrinsic::Pow
            | MathIntrinsic::Tanh
            | MathIntrinsic::Exp2
            | MathIntrinsic::Log2
            | MathIntrinsic::Atan2 => {
                Self::emit_math_libm_call(builder, ctx, intrinsic, ty, is_f32, &arg_values)
            }
            MathIntrinsic::Fract => Self::emit_math_fract(builder, ty, arg_values[0]),
            MathIntrinsic::Clamp => Self::emit_math_clamp(builder, ctx, ty, is_f32, &arg_values),
            MathIntrinsic::Mix => Self::emit_math_mix(builder, ty, &arg_values),
            MathIntrinsic::Smoothstep => {
                Self::emit_math_smoothstep(builder, ctx, ty, is_f32, &arg_values)
            }
            MathIntrinsic::Step => Self::emit_math_step(builder, ty, &arg_values),
            MathIntrinsic::Sign => Self::emit_math_sign(builder, ty, arg_values[0]),
            MathIntrinsic::VecDot
            | MathIntrinsic::VecLength
            | MathIntrinsic::VecNormalize
            | MathIntrinsic::VecCross
            | MathIntrinsic::VecReflect
            | MathIntrinsic::VecMix => Err(CodegenError::Internal(
                "vector builtins are GPU-only and should not be lowered for CPU".to_string(),
            )),
        }
    }

    /// `abs(x)`: native `fabs` for floats; bit-twiddle for integers.
    fn emit_math_abs(builder: &mut FunctionBuilder, ty: cl_types::Type, val: Value) -> Value {
        if ty.is_float() {
            return builder.ins().fabs(val);
        }
        // Integer abs: (x ^ (x >> (bits-1))) - (x >> (bits-1))
        //
        // For the most-negative value (e.g. `i64::MIN`, whose magnitude has no
        // representable positive counterpart) this wraps back to that same value:
        // `x >> 63` is all-ones (-1), so `x ^ -1` is `!MIN == MAX`, and
        // `MAX - (-1) == MAX + 1` overflows two's-complement back to `MIN`.
        // This is deliberate: the result stays in range and is bit-identical to
        // the platform `abs`/WGSL `abs` builtin (see `math_intrinsic_name`), so
        // CPU and GPU agree. It is correct-by-semantics (defined two's-complement
        // wrap), not by mathematics (|MIN| is not representable). Callers that need
        // the mathematically-correct magnitude must widen before taking `abs`.
        let shift = ty.bits() - 1;
        let sign_mask = builder.ins().sshr_imm(val, shift as i64);
        let xor = builder.ins().bxor(val, sign_mask);
        builder.ins().isub(xor, sign_mask)
    }

    /// `sqrt(x)`: native `sqrt` for floats; rejected for integers.
    fn emit_math_sqrt(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        val: Value,
    ) -> Result<Value, CodegenError> {
        if ty.is_float() {
            Ok(builder.ins().sqrt(val))
        } else {
            Err(CodegenError::Internal(
                "sqrt expects a float argument".to_string(),
            ))
        }
    }

    /// `ceil`/`floor`/`round`: emit `op` for floats; pass integers through
    /// unchanged (these are exact-integer operations).
    fn emit_math_unary_int_passthrough(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        val: Value,
        op: impl FnOnce(&mut FunctionBuilder, Value) -> Value,
    ) -> Value {
        if ty.is_float() {
            op(builder, val)
        } else {
            val
        }
    }

    /// `min(a, b)` / `max(a, b)`: float path goes through libm
    /// (`fmin`/`fmax`/`fminf`/`fmaxf`); integers use Cranelift native
    /// `smin`/`smax`. `is_min` selects which.
    fn emit_math_min_max(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ty: cl_types::Type,
        is_f32: bool,
        arg_values: &[Value],
        is_min: bool,
    ) -> Result<Value, CodegenError> {
        if ty.is_float() {
            let func_name = match (is_min, is_f32) {
                (true, true) => "fminf",
                (true, false) => "fmin",
                (false, true) => "fmaxf",
                (false, false) => "fmax",
            };
            return Self::emit_libm_call(builder, ctx, func_name, ty, arg_values);
        }
        if arg_values.len() != 2 {
            return Err(CodegenError::Internal(format!(
                "{} expects exactly two arguments",
                if is_min { "min" } else { "max" }
            )));
        }
        Ok(if is_min {
            builder.ins().smin(arg_values[0], arg_values[1])
        } else {
            builder.ins().smax(arg_values[0], arg_values[1])
        })
    }

    /// Dispatch a libm-routed math intrinsic to the matching `sinf`/`cosf`/etc symbol.
    fn emit_math_libm_call(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        intrinsic: MathIntrinsic,
        ty: cl_types::Type,
        is_f32: bool,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        if !ty.is_float() {
            return Err(CodegenError::Internal(format!(
                "Math intrinsic {} expects float arguments, found {}",
                intrinsic, ty
            )));
        }
        let func_name = match (intrinsic, is_f32) {
            (MathIntrinsic::Sin, true) => "sinf",
            (MathIntrinsic::Sin, false) => "sin",
            (MathIntrinsic::Cos, true) => "cosf",
            (MathIntrinsic::Cos, false) => "cos",
            (MathIntrinsic::Tan, true) => "tanf",
            (MathIntrinsic::Tan, false) => "tan",
            (MathIntrinsic::Ln, true) => "logf",
            (MathIntrinsic::Ln, false) => "log",
            (MathIntrinsic::Exp, true) => "expf",
            (MathIntrinsic::Exp, false) => "exp",
            (MathIntrinsic::Pow, true) => "powf",
            (MathIntrinsic::Pow, false) => "pow",
            (MathIntrinsic::Tanh, true) => "tanhf",
            (MathIntrinsic::Tanh, false) => "tanh",
            (MathIntrinsic::Exp2, true) => "exp2f",
            (MathIntrinsic::Exp2, false) => "exp2",
            (MathIntrinsic::Log2, true) => "log2f",
            (MathIntrinsic::Log2, false) => "log2",
            (MathIntrinsic::Atan2, true) => "atan2f",
            (MathIntrinsic::Atan2, false) => "atan2",
            (MathIntrinsic::Abs, _)
            | (MathIntrinsic::Sqrt, _)
            | (MathIntrinsic::Floor, _)
            | (MathIntrinsic::Ceil, _)
            | (MathIntrinsic::Round, _)
            | (MathIntrinsic::Min, _)
            | (MathIntrinsic::Max, _)
            | (MathIntrinsic::Fract, _)
            | (MathIntrinsic::Clamp, _)
            | (MathIntrinsic::Mix, _)
            | (MathIntrinsic::Smoothstep, _)
            | (MathIntrinsic::Step, _)
            | (MathIntrinsic::Sign, _)
            | (MathIntrinsic::VecDot, _)
            | (MathIntrinsic::VecLength, _)
            | (MathIntrinsic::VecNormalize, _)
            | (MathIntrinsic::VecCross, _)
            | (MathIntrinsic::VecReflect, _)
            | (MathIntrinsic::VecMix, _) => {
                return Err(CodegenError::Internal(format!(
                    "internal codegen error: {:?} routed to libm branch",
                    intrinsic
                )));
            }
        };
        Self::emit_libm_call(builder, ctx, func_name, ty, arg_values)
    }

    /// `fract(x)`: x - floor(x) for floats; identity for integers.
    fn emit_math_fract(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        val: Value,
    ) -> Result<Value, CodegenError> {
        if ty.is_float() {
            let floored = builder.ins().floor(val);
            Ok(builder.ins().fsub(val, floored))
        } else {
            Ok(val)
        }
    }

    /// `clamp(x, lo, hi)`: min(max(x, lo), hi).
    fn emit_math_clamp(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ty: cl_types::Type,
        is_f32: bool,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        if arg_values.len() != 3 {
            return Err(CodegenError::Internal(format!(
                "clamp expects exactly three arguments, got {}",
                arg_values.len()
            )));
        }
        if ty.is_float() {
            let max_val = Self::emit_math_min_max(
                builder,
                ctx,
                ty,
                is_f32,
                &[arg_values[0], arg_values[1]],
                false,
            )?;
            Self::emit_math_min_max(builder, ctx, ty, is_f32, &[max_val, arg_values[2]], true)
        } else {
            let max_val = builder.ins().smax(arg_values[0], arg_values[1]);
            Ok(builder.ins().smin(max_val, arg_values[2]))
        }
    }

    /// `mix(a, b, t)`: a*(1.0-t) + b*t.
    fn emit_math_mix(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        if arg_values.len() != 3 {
            return Err(CodegenError::Internal(format!(
                "mix expects exactly three arguments, got {}",
                arg_values.len()
            )));
        }
        if !ty.is_float() {
            return Err(CodegenError::Internal(
                "mix expects float arguments".to_string(),
            ));
        }
        let a = arg_values[0];
        let b = arg_values[1];
        let t = arg_values[2];
        let one = if ty == cl_types::F32 {
            builder.ins().f32const(1.0f32)
        } else {
            builder.ins().f64const(1.0f64)
        };
        let one_minus_t = builder.ins().fsub(one, t);
        let a_part = builder.ins().fmul(a, one_minus_t);
        let b_part = builder.ins().fmul(b, t);
        Ok(builder.ins().fadd(a_part, b_part))
    }

    /// `smoothstep(low, high, x)`: t = clamp((x-low)/(high-low), 0, 1); t*t*(3.0 - 2.0*t).
    fn emit_math_smoothstep(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ty: cl_types::Type,
        is_f32: bool,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        if arg_values.len() != 3 {
            return Err(CodegenError::Internal(format!(
                "smoothstep expects exactly three arguments, got {}",
                arg_values.len()
            )));
        }
        if !ty.is_float() {
            return Err(CodegenError::Internal(
                "smoothstep expects float arguments".to_string(),
            ));
        }
        let low = arg_values[0];
        let high = arg_values[1];
        let x = arg_values[2];
        let zero = if ty == cl_types::F32 {
            builder.ins().f32const(0.0f32)
        } else {
            builder.ins().f64const(0.0f64)
        };
        let one = if ty == cl_types::F32 {
            builder.ins().f32const(1.0f32)
        } else {
            builder.ins().f64const(1.0f64)
        };
        let two = if ty == cl_types::F32 {
            builder.ins().f32const(2.0f32)
        } else {
            builder.ins().f64const(2.0f64)
        };
        let three = if ty == cl_types::F32 {
            builder.ins().f32const(3.0f32)
        } else {
            builder.ins().f64const(3.0f64)
        };

        let x_minus_low = builder.ins().fsub(x, low);
        let high_minus_low = builder.ins().fsub(high, low);
        let t_unclamped = builder.ins().fdiv(x_minus_low, high_minus_low);

        let t = Self::emit_math_clamp(builder, ctx, ty, is_f32, &[t_unclamped, zero, one])?;

        let t_squared = builder.ins().fmul(t, t);
        let two_t = builder.ins().fmul(two, t);
        let three_minus_two_t = builder.ins().fsub(three, two_t);
        Ok(builder.ins().fmul(t_squared, three_minus_two_t))
    }

    /// `step(edge, x)`: returns 1.0 if x >= edge, else 0.0.
    fn emit_math_step(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        arg_values: &[Value],
    ) -> Result<Value, CodegenError> {
        if arg_values.len() != 2 {
            return Err(CodegenError::Internal(format!(
                "step expects exactly two arguments, got {}",
                arg_values.len()
            )));
        }
        if !ty.is_float() {
            return Err(CodegenError::Internal(
                "step expects float arguments".to_string(),
            ));
        }
        let edge = arg_values[0];
        let x = arg_values[1];
        let zero = if ty == cl_types::F32 {
            builder.ins().f32const(0.0f32)
        } else {
            builder.ins().f64const(0.0f64)
        };
        let one = if ty == cl_types::F32 {
            builder.ins().f32const(1.0f32)
        } else {
            builder.ins().f64const(1.0f64)
        };
        let cmp = builder.ins().fcmp(FloatCC::GreaterThanOrEqual, x, edge);
        Ok(builder.ins().select(cmp, one, zero))
    }

    /// `sign(x)`: returns -1.0, 0.0, or 1.0 based on sign of x.
    fn emit_math_sign(
        builder: &mut FunctionBuilder,
        ty: cl_types::Type,
        val: Value,
    ) -> Result<Value, CodegenError> {
        if !ty.is_float() {
            return Err(CodegenError::Internal(
                "sign expects float arguments".to_string(),
            ));
        }
        let zero = if ty == cl_types::F32 {
            builder.ins().f32const(0.0f32)
        } else {
            builder.ins().f64const(0.0f64)
        };
        let one = if ty == cl_types::F32 {
            builder.ins().f32const(1.0f32)
        } else {
            builder.ins().f64const(1.0f64)
        };
        let neg_one = if ty == cl_types::F32 {
            builder.ins().f32const(-1.0f32)
        } else {
            builder.ins().f64const(-1.0f64)
        };

        let is_negative = builder.ins().fcmp(FloatCC::LessThan, val, zero);
        let is_positive = builder.ins().fcmp(FloatCC::GreaterThan, val, zero);

        let if_negative = builder.ins().select(is_negative, neg_one, zero);
        Ok(builder.ins().select(is_positive, one, if_negative))
    }

    /// Translate an operand to a Cranelift value.
    ///
    /// When `expected_ty` is Some, it is used by `read_place` to resolve
    /// the correct load width for enum/Option field projections.
    pub(crate) fn translate_operand(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected_ty: Option<&crate::ast::types::Type>,
    ) -> Result<Value, CodegenError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::read_place(builder, ctx, place, locals, type_ctx, expected_ty)
            }

            Operand::Constant(constant) => {
                Self::translate_constant(builder, ctx, constant, type_ctx)
            }
        }
    }
    /// Translate a constant to a Cranelift value.
    pub(crate) fn translate_constant(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        constant: &Constant,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let cl_type = translate_type(&constant.ty, ptr_type);

        match &constant.literal {
            Literal::Integer(int_lit) => Self::translate_int_literal(builder, int_lit, cl_type),
            Literal::Float(float_lit) => {
                Ok(Self::translate_float_literal(builder, float_lit, cl_type))
            }
            Literal::Boolean(val) => {
                Ok(builder.ins().iconst(cl_types::I8, if *val { 1 } else { 0 }))
            }
            Literal::None => Ok(builder.ins().iconst(cl_types::I8, 0)),
            Literal::String(s) => Self::translate_string_literal(builder, ctx, s, ptr_type),
            Literal::Identifier(name) => {
                Self::translate_identifier_literal(builder, ctx, name, constant, ptr_type)
            }
            Literal::Regex(_) => Err(CodegenError::Internal(
                "Regex literal reached codegen; MIR lowering must replace all regex literals with calls to Regex.from_validated_pattern()"
                    .to_string(),
            )),
        }
    }

    /// Materialize an integer literal as a Cranelift value. 128-bit literals
    /// build via `iconcat` of lo/hi `I64` halves to avoid truncation; smaller
    /// widths sign-extend to `i64` then `iconst` to the declared `cl_type`.
    fn translate_int_literal(
        builder: &mut FunctionBuilder,
        int_lit: &IntegerLiteral,
        cl_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        match int_lit {
            IntegerLiteral::I128(v) => {
                let lo = (*v as u128 & 0xFFFF_FFFF_FFFF_FFFF) as i64;
                let hi = ((*v as u128) >> 64) as i64;
                let lo_val = builder.ins().iconst(cl_types::I64, lo);
                let hi_val = builder.ins().iconst(cl_types::I64, hi);
                Ok(builder.ins().iconcat(lo_val, hi_val))
            }
            IntegerLiteral::U128(v) => {
                let lo = (*v & 0xFFFF_FFFF_FFFF_FFFF) as i64;
                let hi = (*v >> 64) as i64;
                let lo_val = builder.ins().iconst(cl_types::I64, lo);
                let hi_val = builder.ins().iconst(cl_types::I64, hi);
                Ok(builder.ins().iconcat(lo_val, hi_val))
            }
            IntegerLiteral::I8(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::I16(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::I32(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::I64(v) => Ok(builder.ins().iconst(cl_type, *v)),
            IntegerLiteral::U8(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::U16(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::U32(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
            IntegerLiteral::U64(v) => Ok(builder.ins().iconst(cl_type, *v as i64)),
        }
    }

    /// Materialize a float literal. Uses the declared `cl_type` rather than
    /// the literal's intrinsic type so the value matches the variable
    /// declaration (e.g. an `f64` literal in an `F32` slot).
    fn translate_float_literal(
        builder: &mut FunctionBuilder,
        float_lit: &FloatLiteral,
        cl_type: cl_types::Type,
    ) -> Value {
        let val_f64 = match float_lit {
            FloatLiteral::F32(bits) => f32::from_bits(*bits) as f64,
            FloatLiteral::F64(bits) => f64::from_bits(*bits),
        };
        if cl_type == cl_types::F32 {
            builder.ins().f32const(val_f64 as f32)
        } else {
            builder.ins().f64const(val_f64)
        }
    }

    /// Materialize a string literal as a pointer to an immortal `MiriString`
    /// static data block. The data block is declared once per unique literal
    /// (deduplicated through `ctx.string_literals`); the actual bytes and RC
    /// header are written later by `define_string_literals` in `mod.rs`.
    /// Returns a pointer past the RC header (a valid `*const MiriString`).
    fn translate_string_literal(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        s: &str,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        let ptr_size = ptr_type.bytes() as i32;
        let symbol_name = match ctx.string_literals.get(s) {
            Some(name) => name.clone(),
            None => {
                let next_idx = ctx.string_literals.len();
                let name = format!(".miri_str_{}", next_idx);
                ctx.string_literals.insert(s.to_string(), name.clone());
                name
            }
        };

        let mut struct_symbol = String::with_capacity(symbol_name.len() + 7);
        struct_symbol.push_str(&symbol_name);
        struct_symbol.push_str("_struct");
        let struct_id = ctx
            .module
            .declare_data(&struct_symbol, Linkage::Export, false, false)
            .map_err(|e| CodegenError::declare_function(struct_symbol.clone(), e.to_string()))?;
        let struct_gv = ctx.module.declare_data_in_func(struct_id, builder.func);
        let struct_addr = builder.ins().symbol_value(ptr_type, struct_gv);
        Ok(builder.ins().iadd_imm(struct_addr, ptr_size as i64))
    }

    /// Identifier-typed constants: for function-typed identifiers (lambdas,
    /// named function references), declare the symbol as an import using the
    /// signature carried in the constant's type and return its `func_addr`.
    /// Non-function identifiers fall back to a null pointer placeholder.
    fn translate_identifier_literal(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        constant: &Constant,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        let TypeKind::Function(func_data) = &constant.ty.kind else {
            return Ok(builder.ins().iconst(ptr_type, 0));
        };
        let call_conv = builder.func.signature.call_conv;
        let mut sig = Signature::new(call_conv);
        for param in &func_data.params {
            if let ExpressionKind::Type(param_type, _) = &param.typ.node {
                sig.params
                    .push(AbiParam::new(translate_type(param_type, ptr_type)));
            }
        }
        if let Some(ret_expr) = &func_data.return_type {
            if let ExpressionKind::Type(ret_type, _) = &ret_expr.node {
                if ret_type.kind != TypeKind::Void {
                    sig.returns
                        .push(AbiParam::new(translate_type(ret_type, ptr_type)));
                }
            }
        }
        let func_id = ctx
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(name.to_string(), e.to_string()))?;
        let func_ref = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, func_ref))
    }
    /// Translate a closure aggregate into a heap-allocated closure struct.
    ///
    /// Layout: [raw_ptr(=malloc_ptr)][RC=1][fn_ptr][cap0][cap1]...
    /// The returned value is `payload_ptr` = `raw_ptr + 2*ptr_size`.
    fn translate_closure_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        lambda_name: &str,
        fn_type: &crate::ast::types::Type,
        capture_ops: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let capture_vals: Vec<Value> = capture_ops
            .iter()
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx, None))
            .collect::<Result<_, _>>()?;

        let payload_ptr =
            Self::alloc_closure_payload(builder, ctx, capture_vals.len(), ptr_type, ptr_size)?;

        // Store fn_ptr at payload[0].
        let fn_ptr = Self::declare_closure_fn_ptr(builder, ctx, lambda_name, fn_type, ptr_type)?;
        builder.ins().store(MemFlags::new(), fn_ptr, payload_ptr, 0);

        // Store destructor_ptr at payload[1].
        let dtor_val =
            Self::closure_dtor_ptr(builder, ctx, lambda_name, capture_ops, type_ctx, ptr_type)?;
        builder
            .ins()
            .store(MemFlags::new(), dtor_val, payload_ptr, ptr_size as i32);

        // Store each captured value starting at payload[2].
        // Layout: payload[0]=fn_ptr, payload[1]=dtor_ptr, payload[2+i]=cap_i.
        for (i, val) in capture_vals.into_iter().enumerate() {
            let val_ty = builder.func.dfg.value_type(val);
            let widened =
                if val_ty != ptr_type && val_ty.is_int() && val_ty.bits() < ptr_type.bits() {
                    builder.ins().sextend(ptr_type, val)
                } else {
                    val
                };
            let offset = (2 + i as i32) * ptr_size as i32;
            builder
                .ins()
                .store(MemFlags::new(), widened, payload_ptr, offset);
        }

        Ok(payload_ptr)
    }

    /// Heap-allocate `[malloc_ptr][RC][fn_ptr][dtor_ptr][cap_0..cap_{N-1}]`
    /// for a closure with `n_captures` captures. Initializes RC = 1 and stores
    /// the malloc pointer at offset 0 so `free()` can recover the original
    /// allocation. Records the alloc with `miri_rt_closure_alloc_track` so the
    /// leak detector sees it. Returns `payload_ptr = raw_ptr + 2*ptr_size`.
    fn alloc_closure_payload(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        n_captures: usize,
        ptr_type: cl_types::Type,
        ptr_size: i64,
    ) -> Result<Value, CodegenError> {
        let total_size = (2 + 1 + 1 + n_captures as i64) * ptr_size;
        let size_val = builder.ins().iconst(ptr_type, total_size);
        let raw_ptr = Self::call_libc_malloc(builder, ctx, size_val)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, raw_ptr, null);
        builder.ins().trapnz(is_null, OOM_TRAP_CODE);

        Self::call_rt_closure_alloc_track(builder, ctx)?;

        builder.ins().store(MemFlags::new(), raw_ptr, raw_ptr, 0);
        let one = builder.ins().iconst(ptr_type, 1);
        builder
            .ins()
            .store(MemFlags::new(), one, raw_ptr, ptr_size as i32);

        Ok(builder.ins().iadd_imm(raw_ptr, 2 * ptr_size))
    }

    /// Build the lambda's Cranelift signature (`env_ptr` first, then user
    /// params) and declare it as `Linkage::Import`, returning its address.
    fn declare_closure_fn_ptr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        lambda_name: &str,
        fn_type: &crate::ast::types::Type,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        use cranelift_module::Module;
        let call_conv = builder.func.signature.call_conv;
        let mut sig = cranelift_codegen::ir::Signature::new(call_conv);
        sig.params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        if let crate::ast::types::TypeKind::Function(func_data) = &fn_type.kind {
            for param in &func_data.params {
                if let ExpressionKind::Type(param_type, _) = &param.typ.node {
                    sig.params
                        .push(cranelift_codegen::ir::AbiParam::new(translate_type(
                            param_type, ptr_type,
                        )));
                }
            }
            if let Some(ret_expr) = &func_data.return_type {
                if let ExpressionKind::Type(ret_type, _) = &ret_expr.node {
                    if ret_type.kind != TypeKind::Void {
                        sig.returns
                            .push(cranelift_codegen::ir::AbiParam::new(translate_type(
                                ret_type, ptr_type,
                            )));
                    }
                }
            }
        }
        let func_id = ctx
            .module
            .declare_function(lambda_name, cranelift_module::Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(lambda_name.to_string(), e.to_string()))?;
        let func_ref = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, func_ref))
    }

    /// Resolve the closure's destructor pointer. When any capture is managed,
    /// declare `__dtor_{lambda_name}` and take its address; otherwise return a
    /// null pointer (no RC work needed on drop). The destructor DecRefs all
    /// managed captures when the closure's own RC reaches zero.
    fn closure_dtor_ptr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        lambda_name: &str,
        capture_ops: &[Operand],
        type_ctx: &TypeCtx,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        use cranelift_module::Module;
        let has_managed = capture_ops.iter().any(|op| match op {
            Operand::Copy(p) | Operand::Move(p) => {
                let kind = &type_ctx.local_types[p.local.0].kind;
                super::translator::is_capture_managed(kind)
            }
            Operand::Constant(_) => false,
        });
        if !has_managed {
            return Ok(builder.ins().iconst(ptr_type, 0));
        }
        let dtor_name = format!("__dtor_{}", lambda_name);
        let mut dtor_sig = cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
        dtor_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let dtor_id = ctx
            .module
            .declare_function(&dtor_name, cranelift_module::Linkage::Import, &dtor_sig)
            .map_err(|e| CodegenError::declare_function(dtor_name.clone(), e.to_string()))?;
        let dtor_ref = ctx.module.declare_func_in_func(dtor_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, dtor_ref))
    }

    /// Returns the `TypeKind` of a single operand, consulting either the constant's
    /// type or the local variable's declared type.
    fn first_operand_kind<'op>(
        operand: &'op Operand,
        type_ctx: &'op TypeCtx,
    ) -> Option<&'op TypeKind> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Some(&type_ctx.local_types[place.local.0].kind)
            }
            Operand::Constant(c) => Some(&c.ty.kind),
        }
    }

    /// Translate a binary operation.
    ///
    /// `is_unsigned` indicates whether the operands are unsigned integer types.
    /// This affects comparison direction, division, shift, and widening behavior.
    pub(crate) fn translate_binop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        op: BinOp,
        lhs: Value,
        rhs: Value,
        is_unsigned: bool,
    ) -> Result<Value, CodegenError> {
        let (lhs, rhs, ty) = Self::widen_binop_operands(builder, lhs, rhs, is_unsigned);
        let is_float = ty.is_float();
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                Self::translate_binop_arith(builder, ctx, op, lhs, rhs, ty, is_float, is_unsigned)
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => Ok(
                Self::translate_binop_bitwise(builder, op, lhs, rhs, is_unsigned),
            ),
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Ok(
                Self::translate_binop_cmp(builder, op, lhs, rhs, is_float, is_unsigned),
            ),
            BinOp::Offset => Ok(builder.ins().iadd(lhs, rhs)),
        }
    }

    /// Widen `lhs`/`rhs` to a common Cranelift type so the following binop
    /// instruction sees matched operand widths. Integer pairs use
    /// `uextend`/`sextend`; float pairs use `fpromote`. Returns the matched
    /// pair plus their shared type.
    fn widen_binop_operands(
        builder: &mut FunctionBuilder,
        lhs: Value,
        rhs: Value,
        is_unsigned: bool,
    ) -> (Value, Value, cl_types::Type) {
        let lhs_ty = builder.func.dfg.value_type(lhs);
        let rhs_ty = builder.func.dfg.value_type(rhs);
        if lhs_ty == rhs_ty {
            return (lhs, rhs, lhs_ty);
        }
        if !lhs_ty.is_float() && !rhs_ty.is_float() {
            if lhs_ty.bits() > rhs_ty.bits() {
                let rhs = if is_unsigned {
                    builder.ins().uextend(lhs_ty, rhs)
                } else {
                    builder.ins().sextend(lhs_ty, rhs)
                };
                return (lhs, rhs, lhs_ty);
            }
            let lhs = if is_unsigned {
                builder.ins().uextend(rhs_ty, lhs)
            } else {
                builder.ins().sextend(rhs_ty, lhs)
            };
            return (lhs, rhs, rhs_ty);
        }
        if lhs_ty.is_float() && rhs_ty.is_float() {
            if lhs_ty.bits() > rhs_ty.bits() {
                let rhs = builder.ins().fpromote(lhs_ty, rhs);
                return (lhs, rhs, lhs_ty);
            }
            let lhs = builder.ins().fpromote(rhs_ty, lhs);
            return (lhs, rhs, rhs_ty);
        }
        (lhs, rhs, lhs_ty)
    }

    /// Emits an explicit branch: if `rhs == 0`, call `miri_rt_div_by_zero_panic`
    /// (which prints the runtime error and `_exit(1)`s) then trap as unreachable;
    /// otherwise fall through to the continuation block. Avoids Cranelift `trapz`
    /// so the process terminates via clean exit instead of SIGTRAP/SIGILL — keeps
    /// macOS `ReportCrash` from spawning under parallel test load.
    fn emit_div_by_zero_check(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rhs: Value,
        ty: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let zero = builder.ins().iconst(ty, 0);
        let is_zero = builder.ins().icmp(IntCC::Equal, rhs, zero);

        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, panic_block, &[], cont_block, &[]);

        builder.switch_to_block(panic_block);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::DIV_BY_ZERO_PANIC,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        // `miri_rt_div_by_zero_panic` is `noreturn` semantically; the helper
        // calls `_exit(1)`. Emit a trap here only to terminate the block
        // unreachably so the Cranelift verifier is happy.
        builder.ins().trap(TrapCode::unwrap_user(1));
        builder.seal_block(panic_block);

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);
        Ok(())
    }

    /// Emits an explicit branch: if `rhs == 0`, call `miri_rt_rem_by_zero_panic`
    /// (which prints the runtime error and `_exit(1)`s) then trap as unreachable;
    /// otherwise fall through to the continuation block. Avoids Cranelift `trapz`
    /// so the process terminates via clean exit instead of SIGTRAP/SIGILL — keeps
    /// macOS `ReportCrash` from spawning under parallel test load.
    fn emit_rem_by_zero_check(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rhs: Value,
        ty: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let zero = builder.ins().iconst(ty, 0);
        let is_zero = builder.ins().icmp(IntCC::Equal, rhs, zero);

        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, panic_block, &[], cont_block, &[]);

        builder.switch_to_block(panic_block);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::REM_BY_ZERO_PANIC,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        // `miri_rt_rem_by_zero_panic` is `noreturn` semantically; the helper
        // calls `_exit(1)`. Emit a trap here only to terminate the block
        // unreachably so the Cranelift verifier is happy.
        builder.ins().trap(TrapCode::unwrap_user(1));
        builder.seal_block(panic_block);

        builder.switch_to_block(cont_block);
        builder.seal_block(cont_block);
        Ok(())
    }

    /// Emit `Add`/`Sub`/`Mul`/`Div`/`Rem` for the matched operand pair.
    /// Integer `Div`/`Rem` check for division by zero by calling
    /// `miri_rt_div_by_zero_panic` (a clean `_exit(1)`) rather than emitting a
    /// Cranelift `trapz` hardware-trap instruction. Float `Rem` goes via libm
    /// `fmod`/`fmodf` because Cranelift has no native fp remainder.
    #[allow(clippy::too_many_arguments)]
    fn translate_binop_arith(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        op: BinOp,
        lhs: Value,
        rhs: Value,
        ty: cl_types::Type,
        is_float: bool,
        is_unsigned: bool,
    ) -> Result<Value, CodegenError> {
        let result = match op {
            BinOp::Add if is_float => builder.ins().fadd(lhs, rhs),
            BinOp::Add => builder.ins().iadd(lhs, rhs),
            BinOp::Sub if is_float => builder.ins().fsub(lhs, rhs),
            BinOp::Sub => builder.ins().isub(lhs, rhs),
            BinOp::Mul if is_float => builder.ins().fmul(lhs, rhs),
            BinOp::Mul => builder.ins().imul(lhs, rhs),
            BinOp::Div if is_float => builder.ins().fdiv(lhs, rhs),
            BinOp::Div => {
                Self::emit_div_by_zero_check(builder, ctx, rhs, ty)?;
                if is_unsigned {
                    builder.ins().udiv(lhs, rhs)
                } else {
                    builder.ins().sdiv(lhs, rhs)
                }
            }
            BinOp::Rem if is_float => return Self::emit_float_rem(builder, ctx, ty, lhs, rhs),
            BinOp::Rem => {
                Self::emit_rem_by_zero_check(builder, ctx, rhs, ty)?;
                if is_unsigned {
                    builder.ins().urem(lhs, rhs)
                } else {
                    builder.ins().srem(lhs, rhs)
                }
            }
            BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::Offset => {
                return Err(CodegenError::Internal(format!(
                    "internal codegen error: {:?} routed to arithmetic branch",
                    op
                )))
            }
        };
        Ok(result)
    }

    /// Float remainder via libm `fmod` / `fmodf`. Cranelift has no native fp
    /// remainder instruction; the runtime FFI is the only correct path.
    fn emit_float_rem(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ty: cl_types::Type,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, CodegenError> {
        let func_name: &'static str = if ty == cl_types::F32 { "fmodf" } else { "fmod" };
        let mut sig = cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
        sig.params.push(cranelift_codegen::ir::AbiParam::new(ty));
        sig.params.push(cranelift_codegen::ir::AbiParam::new(ty));
        sig.returns.push(cranelift_codegen::ir::AbiParam::new(ty));

        let func_id = ctx
            .module
            .declare_function(func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[lhs, rhs]);
        Ok(builder.inst_results(call)[0])
    }

    /// Emit `BitAnd`/`BitOr`/`BitXor`/`Shl`/`Shr` for matched operand widths.
    /// `Shr` picks logical vs arithmetic right-shift on signedness.
    fn translate_binop_bitwise(
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: Value,
        rhs: Value,
        is_unsigned: bool,
    ) -> Value {
        match op {
            BinOp::BitAnd => builder.ins().band(lhs, rhs),
            BinOp::BitOr => builder.ins().bor(lhs, rhs),
            BinOp::BitXor => builder.ins().bxor(lhs, rhs),
            BinOp::Shl => builder.ins().ishl(lhs, rhs),
            BinOp::Shr if is_unsigned => builder.ins().ushr(lhs, rhs),
            BinOp::Shr => builder.ins().sshr(lhs, rhs),
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::Offset => unreachable!(
                "translate_binop_bitwise called with non-bitwise op {:?}",
                op
            ),
        }
    }

    /// Emit a comparison binop. Result is `I8` (bool). Picks the right
    /// `FloatCC` / signed-or-unsigned `IntCC` variant per operand kind.
    fn translate_binop_cmp(
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: Value,
        rhs: Value,
        is_float: bool,
        is_unsigned: bool,
    ) -> Value {
        let (fcc, icc) = match op {
            BinOp::Eq => (FloatCC::Equal, IntCC::Equal),
            BinOp::Ne => (FloatCC::NotEqual, IntCC::NotEqual),
            BinOp::Lt => (
                FloatCC::LessThan,
                if is_unsigned {
                    IntCC::UnsignedLessThan
                } else {
                    IntCC::SignedLessThan
                },
            ),
            BinOp::Le => (
                FloatCC::LessThanOrEqual,
                if is_unsigned {
                    IntCC::UnsignedLessThanOrEqual
                } else {
                    IntCC::SignedLessThanOrEqual
                },
            ),
            BinOp::Gt => (
                FloatCC::GreaterThan,
                if is_unsigned {
                    IntCC::UnsignedGreaterThan
                } else {
                    IntCC::SignedGreaterThan
                },
            ),
            BinOp::Ge => (
                FloatCC::GreaterThanOrEqual,
                if is_unsigned {
                    IntCC::UnsignedGreaterThanOrEqual
                } else {
                    IntCC::SignedGreaterThanOrEqual
                },
            ),
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::BitXor
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Offset => {
                unreachable!("translate_binop_cmp called with non-comparison op {:?}", op)
            }
        };
        if is_float {
            builder.ins().fcmp(fcc, lhs, rhs)
        } else {
            builder.ins().icmp(icc, lhs, rhs)
        }
    }
    /// Whether the operand's value is an unsigned integer, so that widening it
    /// fills the new bytes with zeros and comparing it orders it as unsigned.
    ///
    /// The projected type decides this: a place read through a field or an index
    /// has the type of what it reaches, not of the local it starts from. Judging
    /// by the base local instead treats every projected read as signed, so an
    /// unsigned value with its top bit set becomes a negative number — `200`
    /// held in a `u8` field reads back as `-56`.
    pub(crate) fn operand_is_unsigned(operand: &Operand, type_ctx: &TypeCtx) -> bool {
        Self::is_unsigned_type_kind(&Self::operand_projected_kind(operand, type_ctx))
    }

    /// The type an operand's value actually has, resolving field and index
    /// projections rather than reporting the base local's type.
    pub(crate) fn operand_projected_kind(operand: &Operand, type_ctx: &TypeCtx) -> TypeKind {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::resolve_projected_type_kind(place, type_ctx)
            }
            Operand::Constant(constant) => constant.ty.kind.clone(),
        }
    }

    /// Returns the TypeKind of an operand.
    ///
    /// Note: this is the *base* local's type and ignores any projection on the
    /// place. A `Copy(t.0)` whose base local is `Tuple<int>` reports
    /// `Tuple<int>` even though the projected value is an `int`. Callers that
    /// branch on aggregate shape must guard with `operand_has_no_projection`.
    fn operand_type_kind<'b>(operand: &'b Operand, type_ctx: &'b TypeCtx) -> &'b TypeKind {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                &type_ctx.local_types[place.local.0].kind
            }
            Operand::Constant(c) => &c.ty.kind,
        }
    }

    /// True when an operand references the whole base local rather than a
    /// projected component (field / index / deref). Used to gate code paths
    /// that interpret the operand's value as a full aggregate.
    pub fn operand_has_no_projection(operand: &Operand) -> bool {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => place.projection.is_empty(),
            Operand::Constant(_) => true,
        }
    }

    /// Generate structural equality comparison for two tuples.
    /// Compares each field and ANDs the results together.
    fn translate_tuple_equality(
        builder: &mut FunctionBuilder,
        _ctx: &mut ModuleCtx,
        lhs_ptr: Value,
        rhs_ptr: Value,
        element_exprs: &[Expression],
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let tuple_type = TypeKind::Tuple(element_exprs.to_vec());

        // Start with true (all fields equal so far)
        let mut result = builder.ins().iconst(cranelift_codegen::ir::types::I8, 1);

        for i in 0..element_exprs.len() {
            let (offset, cl_ty) = field_layout(&tuple_type, i, type_ctx.type_definitions, ptr_type);

            let lhs_field = builder.ins().load(cl_ty, MemFlags::new(), lhs_ptr, offset);
            let rhs_field = builder.ins().load(cl_ty, MemFlags::new(), rhs_ptr, offset);

            let field_eq = if cl_ty.is_float() {
                builder.ins().fcmp(FloatCC::Equal, lhs_field, rhs_field)
            } else {
                builder.ins().icmp(IntCC::Equal, lhs_field, rhs_field)
            };

            result = builder.ins().band(result, field_eq);
        }

        Ok(result)
    }

    /// Generate structural equality comparison for two struct instances.
    /// Compares each field and ANDs the results together.
    fn translate_struct_equality(
        builder: &mut FunctionBuilder,
        lhs_ptr: Value,
        rhs_ptr: Value,
        struct_type: &TypeKind,
        def: &crate::type_checker::context::StructDefinition,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;

        // Start with true (all fields equal so far)
        let mut result = builder.ins().iconst(cranelift_codegen::ir::types::I8, 1);

        for i in 0..def.fields.len() {
            let (offset, cl_ty) = field_layout(struct_type, i, type_ctx.type_definitions, ptr_type);

            let lhs_field = builder.ins().load(cl_ty, MemFlags::new(), lhs_ptr, offset);
            let rhs_field = builder.ins().load(cl_ty, MemFlags::new(), rhs_ptr, offset);

            let field_eq = if cl_ty.is_float() {
                builder.ins().fcmp(FloatCC::Equal, lhs_field, rhs_field)
            } else {
                builder.ins().icmp(IntCC::Equal, lhs_field, rhs_field)
            };

            result = builder.ins().band(result, field_eq);
        }

        Ok(result)
    }

    /// Translate a unary operation.
    pub(crate) fn translate_unop(
        builder: &mut FunctionBuilder,
        op: UnOp,
        val: Value,
    ) -> Result<Value, CodegenError> {
        let ty = builder.func.dfg.value_type(val);
        let is_float = ty.is_float();

        let result = match op {
            UnOp::Neg => {
                if is_float {
                    builder.ins().fneg(val)
                } else {
                    builder.ins().ineg(val)
                }
            }
            UnOp::Not => {
                if ty == cl_types::I8 {
                    // Logical not for booleans (I8): flip 0↔1 via XOR
                    builder.ins().bxor_imm(val, 1)
                } else {
                    // Bitwise not for integers
                    builder.ins().bnot(val)
                }
            }
            UnOp::Await => {
                return Err(CodegenError::Internal(
                    "Await not supported in synchronous codegen".to_string(),
                ));
            }
        };

        Ok(result)
    }
}
