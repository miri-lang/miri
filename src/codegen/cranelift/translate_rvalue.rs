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

    /// Translate a MIR rvalue to a Cranelift value.
    pub(crate) fn translate_rvalue(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;

        match rvalue {
            Rvalue::Allocate(size_op, align_op, _) => {
                Self::translate_allocate(builder, ctx, size_op, align_op, locals, type_ctx)
            }
            Rvalue::Use(operand) => {
                Self::translate_operand(builder, ctx, operand, locals, type_ctx)
            }

            Rvalue::BinaryOp(op, lhs, rhs) => {
                Self::translate_binary_op(builder, ctx, *op, lhs, rhs, locals, type_ctx)
            }

            Rvalue::UnaryOp(op, operand) => {
                let val = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                Self::translate_unop(builder, *op, val)
            }

            Rvalue::Ref(place) => {
                let value = Self::read_place(builder, ctx, place, locals, type_ctx)?;
                let val_ty = builder.func.dfg.value_type(value);
                let size = val_ty.bytes();
                let align = size; // Simplification for scalars
                let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, size, align as u8);
                let slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(ptr_type, slot, 0);
                builder.ins().store(MemFlags::new(), value, addr, 0);
                Ok(addr)
            }

            Rvalue::Aggregate(kind, operands) => {
                Self::translate_aggregate(builder, ctx, kind, operands, locals, type_ctx)
            }

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                let dest_ty = translate_type(ty, ptr_type);
                let src_ty = builder.func.dfg.value_type(value);
                let is_unsigned = Self::is_unsigned_type_kind(&ty.kind);

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
        }
    }

    /// Translate an `Rvalue::Allocate` to a Cranelift value (raw aligned heap
    /// slot with `[malloc_ptr][RC][payload...]` header).
    fn translate_allocate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        size_op: &Operand,
        align_op: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        let size = Self::translate_operand(builder, ctx, size_op, locals, type_ctx)?;
        let align = Self::translate_operand(builder, ctx, align_op, locals, type_ctx)?;

        // Layout: [padding][malloc_ptr][RC][payload...]
        //
        // Over-allocate so payload can be aligned. RC header lives at
        // (payload - ptr_size); real malloc pointer at (payload - 2*ptr_size).
        let header_overhead = builder.ins().iconst(ptr_type, 2 * ptr_size as i64);
        let total_size = builder.ins().iadd(size, align);
        let total_size = builder.ins().iadd(total_size, header_overhead);

        let raw_ptr = Self::call_libc_malloc(builder, ctx, total_size)?;

        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, raw_ptr, null);
        builder.ins().trapnz(is_null, OOM_TRAP_CODE);

        // Payload starts at align_to(raw_ptr + 2*ptr_size, align)
        let payload_base = builder.ins().iadd(raw_ptr, header_overhead);
        let mask = builder.ins().ineg(align);
        let align_minus_1 = builder.ins().iadd_imm(align, -1);
        let bumped = builder.ins().iadd(payload_base, align_minus_1);
        let ptr = builder.ins().band(bumped, mask);

        let malloc_slot = builder.ins().iadd_imm(ptr, -(2 * ptr_size as i64));
        builder
            .ins()
            .store(MemFlags::new(), raw_ptr, malloc_slot, 0);

        let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
        let one = builder.ins().iconst(ptr_type, 1);
        builder.ins().store(MemFlags::new(), one, header_ptr, 0);

        Ok(ptr)
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

        let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
        let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
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
                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
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
                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
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
    pub(crate) fn translate_aggregate(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &AggregateKind,
        operands: &[Operand],
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
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
        Self::build_struct_like_aggregate(builder, ctx, kind, operands, locals, type_ctx)
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
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx))
            .collect::<Result<_, _>>()?;

        // Determine element size from the first operand (all are homogeneous)
        let elem_size = if translated.is_empty() {
            ptr_size as i64
        } else {
            builder.func.dfg.value_type(translated[0]).bytes() as i64
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

        if !translated.is_empty() {
            // Read data pointer from MiriArray.data (offset 0)
            let data_ptr = builder.ins().load(ptr_type, MemFlags::new(), array_ptr, 0);
            for (i, val) in translated.iter().enumerate() {
                let offset = (i as i64) * elem_size;
                builder
                    .ins()
                    .store(MemFlags::new(), *val, data_ptr, offset as i32);
            }
        }

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
        if let Some(elem_kind) = first_op_direct_kind {
            Self::register_elem_drop_clone(
                builder,
                ctx,
                elem_kind,
                array_ptr,
                ptr_type,
                type_ctx.type_definitions,
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
                    type_ctx.type_definitions,
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
        if key_kind == 1 {
            let drop_fn_addr = Self::get_rt_string_decref_element_addr(builder, ctx, ptr_type)?;
            Self::call_rt_map_set_key_drop_fn(builder, ctx, map_ptr, drop_fn_addr)?;
        }

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
            type_ctx.type_definitions,
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
                    type_ctx.type_definitions,
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
        type_definitions: &HashMap<String, crate::type_checker::context::TypeDefinition>,
        setters: ElementCallbackSetters,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)? {
            (setters.set_drop)(builder, ctx, container_ptr, addr)?;
        }
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
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
            return Self::translate_operand(builder, ctx, &operands[0], locals, type_ctx);
        }

        let translated: Vec<Value> = operands
            .iter()
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx))
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
            matches!(kind, AggregateKind::Enum(_, _)),
            ptr_size as u32,
        );

        let payload_ptr =
            Self::alloc_aggregate_payload(builder, ctx, ptr_type, ptr_size, total_size)?;
        if is_tuple {
            let count = builder.ins().iconst(ptr_type, translated.len() as i64);
            builder.ins().store(MemFlags::new(), count, payload_ptr, 0);
        }
        if let Some(class_name) = vtable_class_name {
            Self::store_vtable_pointer(builder, ctx, &class_name, payload_ptr, ptr_type)?;
        }
        for (i, val) in translated.into_iter().enumerate() {
            builder
                .ins()
                .store(MemFlags::new(), val, payload_ptr, field_offsets[i] as i32);
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

    /// Compute per-field offsets and total payload size for a struct-like aggregate.
    fn compute_aggregate_layout(
        builder: &FunctionBuilder,
        translated: &[Value],
        header_size: u32,
        is_tuple: bool,
        is_enum: bool,
        ptr_size: u32,
    ) -> (Vec<u32>, u32) {
        let mut current_offset: u32 = header_size;
        let mut field_offsets = Vec::with_capacity(translated.len());
        let mut max_align: u32 = if is_tuple { ptr_size } else { 1 };

        for &val in translated {
            let ty = builder.func.dfg.value_type(val);
            let align = if is_enum { ptr_size } else { ty.bytes() };
            max_align = max_align.max(align);

            current_offset = (current_offset + align - 1) & !(align - 1);
            field_offsets.push(current_offset);
            current_offset += if is_enum { ptr_size } else { ty.bytes() };
        }
        let total_size = (current_offset + max_align - 1) & !(max_align - 1);
        (field_offsets, total_size)
    }

    /// Heap-allocate `[malloc_ptr][RC][payload]` for an aggregate, traps on OOM,
    /// and returns the payload pointer (past the 2-slot header). RC is initialized
    /// to 1.
    fn alloc_aggregate_payload(
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

        let ptr = Self::read_place(builder, ctx, place, locals, type_ctx)?;

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
                builder, ctx, arg, locals, type_ctx,
            )?);
        }
        if arg_values.is_empty() {
            return Err(CodegenError::Internal(format!(
                "Math intrinsic {} expects at least one argument",
                intrinsic
            )));
        }
        let ty = builder.func.dfg.value_type(arg_values[0]);
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
            | MathIntrinsic::Pow => {
                Self::emit_math_libm_call(builder, ctx, intrinsic, ty, is_f32, &arg_values)
            }
        }
    }

    /// `abs(x)`: native `fabs` for floats; bit-twiddle for integers.
    fn emit_math_abs(builder: &mut FunctionBuilder, ty: cl_types::Type, val: Value) -> Value {
        if ty.is_float() {
            return builder.ins().fabs(val);
        }
        // Integer abs: (x ^ (x >> (bits-1))) - (x >> (bits-1))
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
            (MathIntrinsic::Abs, _)
            | (MathIntrinsic::Sqrt, _)
            | (MathIntrinsic::Floor, _)
            | (MathIntrinsic::Ceil, _)
            | (MathIntrinsic::Round, _)
            | (MathIntrinsic::Min, _)
            | (MathIntrinsic::Max, _) => {
                return Err(CodegenError::Internal(format!(
                    "internal codegen error: {:?} routed to libm branch",
                    intrinsic
                )));
            }
        };
        Self::emit_libm_call(builder, ctx, func_name, ty, arg_values)
    }

    /// Translate an operand to a Cranelift value.
    pub(crate) fn translate_operand(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::read_place(builder, ctx, place, locals, type_ctx)
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
                "Regex constants not supported in codegen".to_string(),
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
        let next_idx = ctx.string_literals.len();
        let symbol_name = ctx
            .string_literals
            .entry(s.to_string())
            .or_insert_with(|| {
                use std::fmt::Write;
                let mut s = String::with_capacity(20);
                let _ = write!(s, ".miri_str_{}", next_idx);
                s
            })
            .clone();

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
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx))
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
                Self::emit_div_by_zero_check(builder, ctx, rhs, ty)?;
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
    /// Returns true if the operand has an unsigned integer type.
    fn operand_is_unsigned(operand: &Operand, type_ctx: &TypeCtx) -> bool {
        let kind = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                &type_ctx.local_types[place.local.0].kind
            }
            Operand::Constant(c) => &c.ty.kind,
        };
        matches!(
            kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        )
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
    fn operand_has_no_projection(operand: &Operand) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::literal::Literal;
    use crate::ast::types::Type;
    use crate::error::syntax::Span;
    use crate::mir::{Local, Place};

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, Span::default())
    }

    #[test]
    fn operand_has_no_projection_recognizes_bare_locals_and_constants() {
        let bare = Operand::Copy(Place {
            local: Local(0),
            projection: Vec::new(),
        });
        assert!(FunctionTranslator::operand_has_no_projection(&bare));

        let projected = Operand::Copy(Place {
            local: Local(0),
            projection: vec![crate::mir::PlaceElem::Field(0)],
        });
        assert!(!FunctionTranslator::operand_has_no_projection(&projected));

        let constant = Operand::Constant(Box::new(Constant {
            span: Span::default(),
            ty: ty(TypeKind::Int),
            literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(42)),
        }));
        assert!(FunctionTranslator::operand_has_no_projection(&constant));
    }
}
