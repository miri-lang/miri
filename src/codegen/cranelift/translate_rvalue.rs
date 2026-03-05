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
                if operands.is_empty() {
                    return Ok(builder.ins().iconst(ptr_type, 0));
                }

                // Single-element aggregates can be returned directly UNLESS they need
                // pointer-based layout (like enums or structs that might be expected as pointers).
                let needs_pointer_layout = matches!(
                    kind,
                    AggregateKind::Struct(_) | AggregateKind::Class(_) | AggregateKind::Enum(_, _)
                );
                if operands.len() == 1 && !needs_pointer_layout {
                    return Self::translate_operand(builder, ctx, &operands[0], locals, type_ctx);
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

                    // Align current_offset to this field's alignment
                    current_offset = (current_offset + align - 1) & !(align - 1);
                    field_offsets.push(current_offset);
                    current_offset += if is_enum { ptr_size as u32 } else { ty.bytes() };
                }

                // Final size must be a multiple of the max alignment
                let total_size = (current_offset + max_align - 1) & !(max_align - 1);

                // Heap-allocate for now.
                // For Array/List, we add ptr_size for RC header AND ptr_size for LEN header.
                // For others, just ptr_size for RC header.
                let is_collection = matches!(kind, AggregateKind::Array | AggregateKind::List);
                let header_size = if is_collection {
                    2 * ptr_size
                } else {
                    ptr_size
                };

                let alloc_size = builder
                    .ins()
                    .iconst(ptr_type, (total_size + header_size as u32) as i64);
                let raw_ptr = Self::call_libc_malloc(builder, ctx, alloc_size)?;

                // Store RC = 1
                let one = builder.ins().iconst(ptr_type, 1);
                builder.ins().store(MemFlags::new(), one, raw_ptr, 0);

                if is_collection {
                    // Store LEN
                    let len = builder.ins().iconst(ptr_type, operands.len() as i64);
                    builder.ins().store(MemFlags::new(), len, raw_ptr, ptr_size);
                }

                let payload_ptr = builder.ins().iadd_imm(raw_ptr, header_size as i64);

                // Store each field
                for (i, val) in translated.into_iter().enumerate() {
                    builder
                        .ins()
                        .store(MemFlags::new(), val, payload_ptr, field_offsets[i] as i32);
                }

                Ok(payload_ptr)
            }

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                let dest_ty = translate_type(ty, ptr_type);
                let src_ty = builder.func.dfg.value_type(value);

                Self::cast_value(builder, value, src_ty, dest_ty)
            }

            Rvalue::Len(place) => {
                let ty = type_ctx.local_types[place.local.0];
                let is_collection = match &ty.kind {
                    TypeKind::Array(_, _) | TypeKind::List(_) | TypeKind::String => true,
                    TypeKind::Custom(name, _) if name == "Array" || name == "List" => true,
                    _ => false,
                };

                if is_collection {
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

                    // Use a variable to store the length and merge from both branches
                    let len_var = builder.declare_var(ptr_type);

                    builder.ins().brif(is_null, null_bb, &[], load_bb, &[]);

                    // Branch 1: Null pointer, length is 0
                    builder.switch_to_block(null_bb);
                    let zero = builder.ins().iconst(ptr_type, 0);
                    builder.def_var(len_var, zero);
                    builder.ins().jump(merge_bb, &[]);
                    builder.seal_block(null_bb);

                    // Branch 2: Non-null pointer, load length from header
                    builder.switch_to_block(load_bb);
                    let header_offset = match &ty.kind {
                        TypeKind::String => ptr_size,
                        TypeKind::List(_) => ptr_size,
                        TypeKind::Custom(name, _) if name == "List" => ptr_size,
                        _ => -ptr_size,
                    };
                    let len = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), ptr, header_offset);
                    builder.def_var(len_var, len);
                    builder.ins().jump(merge_bb, &[]);
                    builder.seal_block(load_bb);

                    // Back to merge block to continue
                    builder.switch_to_block(merge_bb);
                    builder.seal_block(merge_bb);

                    Ok(builder.use_var(len_var))
                } else {
                    // Non-collection types: return 0 as fallback
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
