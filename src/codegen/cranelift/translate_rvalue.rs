use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
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
            Rvalue::Allocate(size_op, _, _) => {
                let size = Self::translate_operand(builder, ctx, size_op, locals, type_ctx)?;

                // Add ptr_size bytes for header
                let total_size = builder.ins().iadd_imm(size, ptr_size as i64);

                // Call malloc
                let ptr = Self::call_libc_malloc(builder, ctx, total_size)?;

                // Init RC to 1
                let one = builder.ins().iconst(ptr_type, 1);
                builder.ins().store(MemFlags::new(), one, ptr, 0);

                // Return ptr + ptr_size
                Ok(builder.ins().iadd_imm(ptr, ptr_size as i64))
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
                let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, size, 0);
                let slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(ptr_type, slot, 0);
                builder.ins().store(MemFlags::new(), value, addr, 0);
                Ok(addr)
            }

            Rvalue::Aggregate(kind, operands) => {
                if operands.is_empty() {
                    return Ok(builder.ins().iconst(ptr_type, 0));
                }
                // Single-element aggregates can be returned directly, UNLESS they are
                // structs/classes/enums which need pointer-based field access.
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

                // Compute field offsets from operand types
                let mut total_size: u32 = 0;
                let mut field_offsets = Vec::new();
                for &val in &translated {
                    let ty = builder.func.dfg.value_type(val);
                    field_offsets.push(total_size);
                    total_size += ty.bytes();
                }

                // Heap-allocate so the pointer survives returning from functions
                let size_val = builder.ins().iconst(ptr_type, total_size as i64);
                let addr = Self::call_libc_malloc(builder, ctx, size_val)?;

                // Store each field
                for (i, val) in translated.into_iter().enumerate() {
                    builder
                        .ins()
                        .store(MemFlags::new(), val, addr, field_offsets[i] as i32);
                }

                Ok(addr)
            }

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(builder, ctx, operand, locals, type_ctx)?;
                let dest_ty = translate_type(ty, ptr_type);
                let src_ty = builder.func.dfg.value_type(value);

                Self::cast_value(builder, value, src_ty, dest_ty)
            }

            Rvalue::Len(_place) => {
                // For now, return a placeholder length
                // Full implementation would dereference the place and get length
                // This is currently only used in for-loops over lists
                // where we rewrite to while loops with explicit bounds
                Ok(builder.ins().iconst(ptr_type, 0))
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
                // Get Bytes Symbol address (safe code relocation)
                let next_idx = ctx.string_literals.len();
                let params_symbol = ctx
                    .string_literals
                    .entry(s.clone())
                    .or_insert_with(|| format!(".miri_str_{}", next_idx))
                    .clone();

                let bytes_symbol = format!("{}_bytes", params_symbol);
                let bytes_id = ctx
                    .module
                    .declare_data(&bytes_symbol, Linkage::Export, false, false)
                    .map_err(|e| format!("Error declaring string bytes: {}", e))?;
                let bytes_gv = ctx.module.declare_data_in_func(bytes_id, builder.func);
                let bytes_addr = builder.ins().symbol_value(ptr_type, bytes_gv);

                // Use Lazy Init Cache
                let cache_symbol = format!("{}_wrapper_cache", params_symbol);
                let cache_id = ctx
                    .module
                    .declare_data(&cache_symbol, Linkage::Export, true, false)
                    .map_err(|e| format!("Error declaring cache: {}", e))?;
                let cache_gv = ctx.module.declare_data_in_func(cache_id, builder.func);
                let cache_ptr = builder.ins().symbol_value(ptr_type, cache_gv);

                // Load cache
                let cached_val = builder.ins().load(ptr_type, MemFlags::new(), cache_ptr, 0);

                let zero = builder.ins().iconst(ptr_type, 0);
                let is_zero = builder.ins().icmp(IntCC::Equal, cached_val, zero);

                let init_block = builder.create_block();
                let continue_block = builder.create_block();

                builder
                    .ins()
                    .brif(is_zero, init_block, &[], continue_block, &[]);

                builder.switch_to_block(init_block);

                // Alloc RC header (ptr_size bytes) + MiriString (3 * ptr_size bytes) = 4 * ptr_size bytes
                // Layout: [RC header @ 0] [MiriString @ ptr_size]
                // This matches the Rvalue::Allocate convention where the
                // returned pointer (raw_ptr + ptr_size) is a valid *const MiriString
                // that can be passed directly to runtime C functions, while
                // IncRef/DecRef access the header at (ptr - ptr_size).
                let alloc_size = builder.ins().iconst(ptr_type, (ptr_size * 4) as i64);
                let raw_ptr = Self::call_libc_malloc(builder, ctx, alloc_size)?;

                // Offset 0: RC header (immortal)
                let header = if ptr_type == cl_types::I32 {
                    builder.ins().iconst(ptr_type, 1 << 28)
                } else {
                    builder.ins().iconst(ptr_type, 1 << 60)
                };
                builder.ins().store(MemFlags::new(), header, raw_ptr, 0);

                // Offset ptr_size: MiriString.data (pointer to string bytes)
                builder
                    .ins()
                    .store(MemFlags::new(), bytes_addr, raw_ptr, ptr_size);
                // Offset 2 * ptr_size: MiriString.len
                let len_val = builder.ins().iconst(ptr_type, s.len() as i64);
                builder
                    .ins()
                    .store(MemFlags::new(), len_val, raw_ptr, 2 * ptr_size);
                // Offset 3 * ptr_size: MiriString.capacity (same as len for literals)
                builder
                    .ins()
                    .store(MemFlags::new(), len_val, raw_ptr, 3 * ptr_size);

                // Update Cache
                builder.ins().store(MemFlags::new(), raw_ptr, cache_ptr, 0);

                builder.ins().jump(continue_block, &[]);
                builder.seal_block(init_block);

                builder.switch_to_block(continue_block);
                // Load cached raw_ptr
                let cached_raw = builder.ins().load(ptr_type, MemFlags::new(), cache_ptr, 0);

                // Return pointer past header — a valid *const MiriString
                Ok(builder.ins().iadd_imm(cached_raw, ptr_size as i64))
            }

            Literal::Symbol(_) => {
                // Symbols are represented as pointer-sized integers
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
