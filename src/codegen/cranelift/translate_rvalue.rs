use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{AggregateKind, BinOp, Constant, Local, Operand, Rvalue, UnOp};
use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types as cl_types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, TrapCode, Value,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

impl<'a> FunctionTranslator<'a> {
    /// Translate a MIR rvalue to a Cranelift value.
    pub(crate) fn translate_rvalue(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;

        match rvalue {
            Rvalue::Allocate(size_op, align_op, _) => {
                let size = Self::translate_operand(builder, ctx, size_op, locals, type_ctx)?;
                let align = Self::translate_operand(builder, ctx, align_op, locals, type_ctx)?;

                // For now, we use libc malloc which doesn't take alignment directly.
                // We'd need aligned_alloc for full support.
                // However, we MUST still ensure the RC header doesn't break alignment.
                // We'll over-allocate and align the payload.

                // Header is ptr_size
                let total_size = builder.ins().iadd(size, align);
                let total_size = builder.ins().iadd_imm(total_size, ptr_size as i64);

                let raw_ptr = Self::call_libc_malloc(builder, ctx, total_size)?;

                // Payload starts at align_to(raw_ptr + ptr_size, align)
                let payload_base = builder.ins().iadd_imm(raw_ptr, ptr_size as i64);
                let mask = builder.ins().ineg(align);
                let align_minus_1 = builder.ins().iadd_imm(align, -1);
                let bumped = builder.ins().iadd(payload_base, align_minus_1);
                let ptr = builder.ins().band(bumped, mask);

                // RC header lives at (ptr - ptr_size)
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                let one = builder.ins().iconst(ptr_type, 1);
                builder.ins().store(MemFlags::new(), one, header_ptr, 0);

                Ok(ptr)
            }
            Rvalue::Use(operand) => {
                Self::translate_operand(builder, ctx, operand, locals, type_ctx)
            }

            Rvalue::BinaryOp(op, lhs, rhs) => {
                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
                Self::translate_binop(builder, *op, lhs_val, rhs_val)
            }

            Rvalue::UnaryOp(op, operand) => {
                let val = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                Self::translate_unop(builder, *op, val)
            }

            Rvalue::Ref(place) => {
                let value = Self::read_place(builder, place, locals, type_ctx)?;
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
                let is_collection = matches!(
                    kind,
                    AggregateKind::Array
                        | AggregateKind::List
                        | AggregateKind::Map
                        | AggregateKind::Set
                );

                if is_collection {
                    // Use runtime struct allocation for Array and List.
                    // Both MiriArray and MiriList are #[repr(C)] structs with:
                    //   offset 0: data pointer
                    //   offset ptr_size: element count / length
                    // The runtime manages the backing memory.

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
                        AggregateKind::Array => {
                            let count_val = builder.ins().iconst(ptr_type, operands.len() as i64);
                            let array_ptr =
                                Self::call_rt_array_new(builder, ctx, count_val, elem_size_val)?;

                            if !translated.is_empty() {
                                // Read data pointer from MiriArray.data (offset 0)
                                let data_ptr =
                                    builder.ins().load(ptr_type, MemFlags::new(), array_ptr, 0);

                                // Store each element directly into the data buffer
                                for (i, val) in translated.into_iter().enumerate() {
                                    let offset = (i as i64) * elem_size;
                                    builder.ins().store(
                                        MemFlags::new(),
                                        val,
                                        data_ptr,
                                        offset as i32,
                                    );
                                }
                            }

                            Ok(array_ptr)
                        }
                        AggregateKind::List => {
                            let list_ptr = Self::call_rt_list_new(builder, ctx, elem_size_val)?;

                            // Push each element via runtime call
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

                            Ok(list_ptr)
                        }
                        AggregateKind::Map => {
                            // Map operands alternate: key1, val1, key2, val2, ...
                            // Determine key and value sizes from the first pair
                            let key_size = if translated.len() >= 2 {
                                builder.func.dfg.value_type(translated[0]).bytes() as i64
                            } else {
                                ptr_size as i64
                            };
                            let value_size = if translated.len() >= 2 {
                                builder.func.dfg.value_type(translated[1]).bytes() as i64
                            } else {
                                ptr_size as i64
                            };
                            // Determine key_kind: 1 for string keys, 0 for value keys.
                            // Map literal keys are always constants, so we check the first key's type.
                            let key_kind = if !operands.is_empty() {
                                if let Operand::Constant(c) = &operands[0] {
                                    if matches!(c.ty.kind, TypeKind::String) {
                                        1i64
                                    } else {
                                        0i64
                                    }
                                } else {
                                    0i64
                                }
                            } else {
                                0i64
                            };

                            let key_size_val = builder.ins().iconst(ptr_type, key_size);
                            let value_size_val = builder.ins().iconst(ptr_type, value_size);
                            let key_kind_val = builder.ins().iconst(ptr_type, key_kind);

                            let map_ptr = Self::call_rt_map_new(
                                builder,
                                ctx,
                                key_size_val,
                                value_size_val,
                                key_kind_val,
                            )?;

                            // Insert each key-value pair
                            for chunk in translated.chunks(2) {
                                if chunk.len() == 2 {
                                    let key_val = Self::widen_to_ptr(builder, chunk[0], ptr_type);
                                    let val_val = Self::widen_to_ptr(builder, chunk[1], ptr_type);
                                    Self::call_rt_map_set(builder, ctx, map_ptr, key_val, val_val)?;
                                }
                            }

                            Ok(map_ptr)
                        }
                        AggregateKind::Set => {
                            let set_ptr = Self::call_rt_set_new(builder, ctx, elem_size_val)?;

                            for val in translated {
                                let widened = Self::widen_to_ptr(builder, val, ptr_type);
                                Self::call_rt_set_add(builder, ctx, set_ptr, widened)?;
                            }

                            Ok(set_ptr)
                        }
                        _ => unreachable!(),
                    }
                } else {
                    // Non-collection aggregates (Tuple, Struct, Class, Enum, etc.)
                    if operands.is_empty() {
                        return Ok(builder.ins().iconst(ptr_type, 0));
                    }

                    // Single-element aggregates can be returned directly UNLESS they need
                    // pointer-based layout (like enums or structs expected as pointers).
                    let needs_pointer_layout = matches!(
                        kind,
                        AggregateKind::Struct(_)
                            | AggregateKind::Class(_)
                            | AggregateKind::Enum(_, _)
                    );
                    if operands.len() == 1 && !needs_pointer_layout {
                        return Self::translate_operand(
                            builder,
                            ctx,
                            &operands[0],
                            locals,
                            type_ctx,
                        );
                    }

                    // Translate all operands first
                    let translated: Vec<Value> = operands
                        .iter()
                        .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx))
                        .collect::<Result<_, _>>()?;

                    // Compute field offsets with proper alignment
                    let mut current_offset: u32 = 0;
                    let mut field_offsets = Vec::new();
                    let mut max_align: u32 = 1;

                    let is_enum = matches!(kind, AggregateKind::Enum(_, _));

                    for &val in &translated {
                        let ty = builder.func.dfg.value_type(val);
                        let align = if is_enum { ptr_size as u32 } else { ty.bytes() };
                        max_align = max_align.max(align);

                        current_offset = (current_offset + align - 1) & !(align - 1);
                        field_offsets.push(current_offset);
                        current_offset += if is_enum { ptr_size as u32 } else { ty.bytes() };
                    }

                    let total_size = (current_offset + max_align - 1) & !(max_align - 1);

                    // Heap-allocate with RC header
                    let alloc_size = builder
                        .ins()
                        .iconst(ptr_type, (total_size + ptr_size as u32) as i64);
                    let raw_ptr = Self::call_libc_malloc(builder, ctx, alloc_size)?;

                    // Store RC = 1
                    let one = builder.ins().iconst(ptr_type, 1);
                    builder.ins().store(MemFlags::new(), one, raw_ptr, 0);

                    let payload_ptr = builder.ins().iadd_imm(raw_ptr, ptr_size as i64);

                    for (i, val) in translated.into_iter().enumerate() {
                        builder.ins().store(
                            MemFlags::new(),
                            val,
                            payload_ptr,
                            field_offsets[i] as i32,
                        );
                    }

                    Ok(payload_ptr)
                }
            }

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                let dest_ty = translate_type(ty, ptr_type);
                let src_ty = builder.func.dfg.value_type(value);

                Self::cast_value(builder, value, src_ty, dest_ty)
            }

            Rvalue::Len(place) => {
                let ty = type_ctx.local_types[place.local.0];

                if Self::is_collection_type(&ty.kind) {
                    let ptr = Self::read_place(builder, place, locals, type_ctx)?;

                    // Handle null pointer (empty collection)
                    let is_null = builder.ins().icmp_imm(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        ptr,
                        0,
                    );

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

                    // MiriArray.elem_count, MiriList.len, MiriSet.len are at offset ptr_size.
                    // MiriMap.len is at offset 3*ptr_size (after states, keys, values).
                    builder.switch_to_block(load_bb);
                    let len_offset = if Self::is_map_type(&ty.kind) {
                        ptr_size * 3
                    } else {
                        ptr_size
                    };
                    let len = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), ptr, len_offset);
                    builder.def_var(len_var, len);
                    builder.ins().jump(merge_bb, &[]);
                    builder.seal_block(load_bb);

                    builder.switch_to_block(merge_bb);
                    builder.seal_block(merge_bb);

                    Ok(builder.use_var(len_var))
                } else if matches!(&ty.kind, TypeKind::String) {
                    let ptr = Self::read_place(builder, place, locals, type_ctx)?;

                    // String uses the MiriString layout: [RC][DataPtr][Len][Cap]
                    // ptr points past the RC header, so Len is at offset ptr_size*2
                    // Actually, for strings the pointer past RC header has DataPtr at 0,
                    // Len at ptr_size, Cap at 2*ptr_size
                    let is_null = builder.ins().icmp_imm(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        ptr,
                        0,
                    );
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
                    let len = builder.ins().load(ptr_type, MemFlags::new(), ptr, ptr_size);
                    builder.def_var(len_var, len);
                    builder.ins().jump(merge_bb, &[]);
                    builder.seal_block(load_bb);

                    builder.switch_to_block(merge_bb);
                    builder.seal_block(merge_bb);

                    Ok(builder.use_var(len_var))
                } else {
                    Ok(builder.ins().iconst(ptr_type, 0))
                }
            }

            Rvalue::GpuIntrinsic(_intrinsic) => {
                Err("GPU intrinsics not supported in CPU backend".to_string())
            }

            Rvalue::Phi(_) => Err(
                "Phi nodes must be eliminated before codegen. Run SSA destruction pass."
                    .to_string(),
            ),
        }
    }
    /// Translate an operand to a Cranelift value.
    pub(crate) fn translate_operand(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, String> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::read_place(builder, place, locals, type_ctx)
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
    ) -> Result<Value, String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let cl_type = translate_type(&constant.ty, ptr_type);

        match &constant.literal {
            Literal::Integer(int_lit) => {
                let val = match int_lit {
                    IntegerLiteral::I8(v) => *v as i64,
                    IntegerLiteral::I16(v) => *v as i64,
                    IntegerLiteral::I32(v) => *v as i64,
                    IntegerLiteral::I64(v) => *v,
                    IntegerLiteral::I128(v) => *v as i64,
                    IntegerLiteral::U8(v) => *v as i64,
                    IntegerLiteral::U16(v) => *v as i64,
                    IntegerLiteral::U32(v) => *v as i64,
                    IntegerLiteral::U64(v) => *v as i64,
                    IntegerLiteral::U128(v) => *v as i64,
                };
                Ok(builder.ins().iconst(cl_type, val))
            }

            Literal::Float(float_lit) => {
                // Use the declared type (cl_type) not the literal's intrinsic type
                // to ensure the value matches the variable declaration
                let val_f64 = match float_lit {
                    FloatLiteral::F32(bits) => f32::from_bits(*bits) as f64,
                    FloatLiteral::F64(bits) => f64::from_bits(*bits),
                };

                if cl_type == cl_types::F32 {
                    Ok(builder.ins().f32const(val_f64 as f32))
                } else {
                    // Default to F64 for TypeKind::Float and TypeKind::F64
                    Ok(builder.ins().f64const(val_f64))
                }
            }

            Literal::Boolean(val) => {
                let int_val = if *val { 1i64 } else { 0i64 };
                Ok(builder.ins().iconst(cl_types::I8, int_val))
            }

            Literal::None => {
                // Unit/None is represented as 0
                Ok(builder.ins().iconst(cl_types::I8, 0))
            }

            Literal::String(s) => {
                let next_idx = ctx.string_literals.len();
                let symbol_name = ctx
                    .string_literals
                    .entry(s.clone())
                    .or_insert_with(|| format!(".miri_str_{}", next_idx))
                    .clone();

                let struct_symbol = format!("{}_struct", symbol_name);
                let struct_id = ctx
                    .module
                    .declare_data(&struct_symbol, Linkage::Export, false, false)
                    .map_err(|e| format!("Error declaring string struct: {}", e))?;
                let struct_gv = ctx.module.declare_data_in_func(struct_id, builder.func);
                let struct_addr = builder.ins().symbol_value(ptr_type, struct_gv);

                // Return pointer past RC header (at offset ptr_size) — a valid *const MiriString
                Ok(builder.ins().iadd_imm(struct_addr, ptr_size as i64))
            }

            Literal::Identifier(_) => {
                // Identifiers are represented as pointer-sized integers
                Ok(builder.ins().iconst(ptr_type, 0))
            }

            Literal::Regex(_) => Err("Regex constants not supported in codegen".to_string()),
        }
    }
    /// Translate a binary operation.
    pub(crate) fn translate_binop(
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, String> {
        let lhs_ty = builder.func.dfg.value_type(lhs);
        let rhs_ty = builder.func.dfg.value_type(rhs);

        // Ensure both operands have the same type by widening the smaller one.
        // Float operands are promoted (fpromote); integer operands are sign-extended
        // (sextend) because Miri's integer types default to signed semantics.
        let (lhs, rhs, ty) = if lhs_ty != rhs_ty && !lhs_ty.is_float() && !rhs_ty.is_float() {
            // Integer widths differ — sign-extend the narrower operand.
            if lhs_ty.bits() > rhs_ty.bits() {
                let rhs = builder.ins().sextend(lhs_ty, rhs);
                (lhs, rhs, lhs_ty)
            } else {
                let lhs = builder.ins().sextend(rhs_ty, lhs);
                (lhs, rhs, rhs_ty)
            }
        } else if lhs_ty != rhs_ty && lhs_ty.is_float() && rhs_ty.is_float() {
            // Float widths differ (e.g. F32 literal used in an F64 expression) —
            // promote the narrower float to the wider one.
            if lhs_ty.bits() > rhs_ty.bits() {
                let rhs = builder.ins().fpromote(lhs_ty, rhs);
                (lhs, rhs, lhs_ty)
            } else {
                let lhs = builder.ins().fpromote(rhs_ty, lhs);
                (lhs, rhs, rhs_ty)
            }
        } else {
            (lhs, rhs, lhs_ty)
        };
        let is_float = ty.is_float();

        let result = match op {
            BinOp::Add => {
                if is_float {
                    builder.ins().fadd(lhs, rhs)
                } else {
                    builder.ins().iadd(lhs, rhs)
                }
            }
            BinOp::Sub => {
                if is_float {
                    builder.ins().fsub(lhs, rhs)
                } else {
                    builder.ins().isub(lhs, rhs)
                }
            }
            BinOp::Mul => {
                if is_float {
                    builder.ins().fmul(lhs, rhs)
                } else {
                    builder.ins().imul(lhs, rhs)
                }
            }
            BinOp::Div => {
                if is_float {
                    builder.ins().fdiv(lhs, rhs)
                } else {
                    // Check for division by zero
                    builder.ins().trapz(rhs, TrapCode::INTEGER_DIVISION_BY_ZERO);
                    // Signed division
                    builder.ins().sdiv(lhs, rhs)
                }
            }
            BinOp::Rem => {
                if is_float {
                    return Err("Floating point remainder not directly supported".to_string());
                } else {
                    // Check for division by zero
                    builder.ins().trapz(rhs, TrapCode::INTEGER_DIVISION_BY_ZERO);
                    builder.ins().srem(lhs, rhs)
                }
            }
            BinOp::BitAnd => builder.ins().band(lhs, rhs),
            BinOp::BitOr => builder.ins().bor(lhs, rhs),
            BinOp::BitXor => builder.ins().bxor(lhs, rhs),
            BinOp::Shl => builder.ins().ishl(lhs, rhs),
            BinOp::Shr => builder.ins().sshr(lhs, rhs),

            // Comparison operations return I8 (bool)
            BinOp::Eq => {
                if is_float {
                    builder.ins().fcmp(FloatCC::Equal, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::Equal, lhs, rhs)
                }
            }
            BinOp::Ne => {
                if is_float {
                    builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::NotEqual, lhs, rhs)
                }
            }
            BinOp::Lt => {
                if is_float {
                    builder.ins().fcmp(FloatCC::LessThan, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs)
                }
            }
            BinOp::Le => {
                if is_float {
                    builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs)
                }
            }
            BinOp::Gt => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs)
                }
            }
            BinOp::Ge => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs)
                } else {
                    builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs)
                }
            }
            BinOp::Offset => {
                // Pointer offset
                builder.ins().iadd(lhs, rhs)
            }
        };

        Ok(result)
    }
    /// Translate a unary operation.
    pub(crate) fn translate_unop(
        builder: &mut FunctionBuilder,
        op: UnOp,
        val: Value,
    ) -> Result<Value, String> {
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
                return Err("Await not supported in synchronous codegen".to_string());
            }
        };

        Ok(result)
    }
}
