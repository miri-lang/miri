// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::{Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{
    BasicBlock, BinOp, Body, Constant, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind, UnOp,
};
use crate::type_checker::context::TypeDefinition;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{
    AbiParam, Block, Function, InstBuilder, MemFlags, Signature, StackSlotData, StackSlotKind,
    TrapCode, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use std::collections::HashMap;
use std::sync::Arc;

/// Translates MIR functions to Cranelift IR.
///
/// Each `FunctionTranslator` handles a single function, managing local variables,
/// basic blocks, and the translation of statements and terminators.
pub struct FunctionTranslator {
    /// The Cranelift function being built.
    func: Function,
    /// Function builder context (reusable across functions).
    builder_ctx: FunctionBuilderContext,
    /// Default calling convention for the target.
    call_conv: CallConv,
    /// The types of MIR locals (for type information during translation).
    local_types: Vec<Type>,
    /// Type definitions from the type checker (for layout computation).
    type_definitions: HashMap<String, TypeDefinition>,
}

impl FunctionTranslator {
    /// Create a new function translator.
    pub fn new(
        isa: &Arc<dyn TargetIsa>,
        body: &Body,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Self {
        let func = Function::new();
        let builder_ctx = FunctionBuilderContext::new();

        // Cache local types
        let local_types = body.local_decls.iter().map(|d| d.ty.clone()).collect();

        Self {
            func,
            builder_ctx,
            call_conv: isa.default_call_conv(),
            local_types,
            type_definitions: type_definitions.clone(),
        }
    }

    /// Translate a MIR function body to Cranelift IR.
    pub fn translate(&mut self, body: &Body) -> Result<(), String> {
        // Build the function signature
        self.build_signature(body)?;

        // Create function builder
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        // Keep track of locals and blocks
        let mut locals: HashMap<Local, Variable> = HashMap::new();
        let mut blocks: HashMap<BasicBlock, Block> = HashMap::new();

        // Declare all local variables
        for (idx, local_decl) in body.local_decls.iter().enumerate() {
            let local = Local(idx);
            let cl_type = translate_type(&local_decl.ty);
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

            // Translate all statements
            for stmt in &block_data.statements {
                Self::translate_statement(
                    &mut builder,
                    stmt,
                    &locals,
                    &self.local_types,
                    &self.type_definitions,
                )?;
            }

            // Translate the terminator
            if let Some(ref terminator) = block_data.terminator {
                Self::translate_terminator(
                    &mut builder,
                    terminator,
                    body,
                    &locals,
                    &blocks,
                    &self.local_types,
                    &self.type_definitions,
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
    fn build_signature(&mut self, body: &Body) -> Result<(), String> {
        self.func.signature.call_conv = self.call_conv;

        // Return type is local 0
        if !body.local_decls.is_empty() {
            let ret_ty = &body.local_decls[0].ty;
            if ret_ty.kind != TypeKind::Void {
                let cl_type = translate_type(ret_ty);
                self.func.signature.returns.push(AbiParam::new(cl_type));
            }
        }

        // Parameters are locals 1..=arg_count
        for i in 1..=body.arg_count {
            if i < body.local_decls.len() {
                let param_ty = &body.local_decls[i].ty;
                let cl_type = translate_type(param_ty);
                self.func.signature.params.push(AbiParam::new(cl_type));
            }
        }

        Ok(())
    }

    /// Translate a MIR statement.
    fn translate_statement(
        builder: &mut FunctionBuilder,
        stmt: &Statement,
        locals: &HashMap<Local, Variable>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), String> {
        match &stmt.kind {
            StatementKind::IncRef(place) => {
                let ptr = Self::read_place(builder, place, locals, local_types, type_definitions)?;
                // For now, assume ptr points to data preceded by 8-byte refcount
                // Header is at ptr - 8

                // TODO: Valid pointer check?
                let header_ptr = builder.ins().iadd_imm(ptr, -8);
                let rc = builder.ins().load(
                    cl_types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    header_ptr,
                    0,
                );
                let new_rc = builder.ins().iadd_imm(rc, 1);
                builder.ins().store(
                    cranelift_codegen::ir::MemFlags::new(),
                    new_rc,
                    header_ptr,
                    0,
                );
            }
            StatementKind::DecRef(place) => {
                let ptr = Self::read_place(builder, place, locals, local_types, type_definitions)?;
                let header_ptr = builder.ins().iadd_imm(ptr, -8);
                let rc = builder.ins().load(
                    cl_types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    header_ptr,
                    0,
                );
                let new_rc = builder.ins().iadd_imm(rc, -1);
                builder.ins().store(
                    cranelift_codegen::ir::MemFlags::new(),
                    new_rc,
                    header_ptr,
                    0,
                );

                // If rc == 0, free
                let zero = builder.ins().iconst(cl_types::I64, 0);
                let is_zero = builder.ins().icmp(IntCC::Equal, new_rc, zero);

                let then_block = builder.create_block();
                let else_block = builder.create_block();

                builder
                    .ins()
                    .brif(is_zero, then_block, &[], else_block, &[]);

                builder.switch_to_block(then_block);
                // Call free(header_ptr)
                // We need to look up or define 'free'
                // This requires access to the module/imports which we don't clean have here yet.
                // Assuming a helper `trans.call_free(builder, header_ptr)`
                // For this exercise, we will assume a placeholder helper or panic if not available
                // But we can try to define it inline if we have the signature.
                // See `call_libc` helper below.
                Self::call_libc_free(builder, header_ptr)?;

                builder.ins().jump(else_block, &[]);
                builder.seal_block(then_block);
                builder.switch_to_block(else_block);
                builder.seal_block(else_block);
            }
            StatementKind::Dealloc(place) => {
                let ptr = Self::read_place(builder, place, locals, local_types, type_definitions)?;
                let header_ptr = builder.ins().iadd_imm(ptr, -8);
                Self::call_libc_free(builder, header_ptr)?;
            }
            StatementKind::Assign(place, rvalue) => {
                let mut value =
                    Self::translate_rvalue(builder, rvalue, locals, local_types, type_definitions)?;

                // Handle implicit casts (e.g. float -> f32, i8 -> i32)
                let dest_ty = &local_types[place.local.0];
                let dest_cl_ty = translate_type(dest_ty);
                let val_ty = builder.func.dfg.value_type(value);

                if dest_cl_ty != val_ty {
                    value = Self::cast_value(builder, value, val_ty, dest_cl_ty)?;
                }

                Self::assign_to_place(
                    builder,
                    place,
                    value,
                    locals,
                    local_types,
                    type_definitions,
                )?;
            }
            StatementKind::Nop => {
                // Nothing to do
            }
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) => {
                // These are hints for the optimizer, we can ignore them for now
            }
        }
        Ok(())
    }

    /// Translate a MIR rvalue to a Cranelift value.
    fn translate_rvalue(
        builder: &mut FunctionBuilder,
        rvalue: &Rvalue,
        locals: &HashMap<Local, Variable>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<Value, String> {
        match rvalue {
            Rvalue::Allocate(size_op, _, _) => {
                // Ignore align for now, standard malloc alignment is usually sufficient
                let size = Self::translate_operand(
                    builder,
                    size_op,
                    locals,
                    local_types,
                    type_definitions,
                )?;

                // Add 8 bytes for header
                let total_size = builder.ins().iadd_imm(size, 8);

                // Call malloc
                let ptr = Self::call_libc_malloc(builder, total_size)?;

                // Init RC to 1
                let one = builder.ins().iconst(cl_types::I64, 1);
                builder
                    .ins()
                    .store(cranelift_codegen::ir::MemFlags::new(), one, ptr, 0);

                // Return ptr + 8
                Ok(builder.ins().iadd_imm(ptr, 8))
            }
            Rvalue::Use(operand) => {
                Self::translate_operand(builder, operand, locals, local_types, type_definitions)
            }

            Rvalue::BinaryOp(op, lhs, rhs) => {
                let lhs_val =
                    Self::translate_operand(builder, lhs, locals, local_types, type_definitions)?;
                let rhs_val =
                    Self::translate_operand(builder, rhs, locals, local_types, type_definitions)?;
                Self::translate_binop(builder, *op, lhs_val, rhs_val)
            }

            Rvalue::UnaryOp(op, operand) => {
                let val = Self::translate_operand(
                    builder,
                    operand,
                    locals,
                    local_types,
                    type_definitions,
                )?;
                Self::translate_unop(builder, *op, val)
            }

            Rvalue::Ref(place) => {
                let value =
                    Self::read_place(builder, place, locals, local_types, type_definitions)?;
                let ty = builder.func.dfg.value_type(value);
                let size = ty.bytes();
                let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, size, 0);
                let slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(cl_types::I64, slot, 0);
                builder.ins().store(MemFlags::new(), value, addr, 0);
                Ok(addr)
            }

            Rvalue::Aggregate(_kind, operands) => {
                if operands.is_empty() {
                    return Ok(builder.ins().iconst(cl_types::I64, 0));
                }
                if operands.len() == 1 {
                    return Self::translate_operand(
                        builder,
                        &operands[0],
                        locals,
                        local_types,
                        type_definitions,
                    );
                }

                // Translate all operands first
                let translated: Vec<Value> = operands
                    .iter()
                    .map(|op| {
                        Self::translate_operand(builder, op, locals, local_types, type_definitions)
                    })
                    .collect::<Result<_, _>>()?;

                // Compute field offsets from operand types
                let mut total_size: u32 = 0;
                let mut field_offsets = Vec::new();
                for &val in &translated {
                    let ty = builder.func.dfg.value_type(val);
                    field_offsets.push(total_size);
                    total_size += ty.bytes();
                }

                // Allocate stack slot
                let slot_data = StackSlotData::new(StackSlotKind::ExplicitSlot, total_size, 0);
                let slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(cl_types::I64, slot, 0);

                // Store each field
                for (i, val) in translated.into_iter().enumerate() {
                    builder
                        .ins()
                        .store(MemFlags::new(), val, addr, field_offsets[i] as i32);
                }

                Ok(addr)
            }

            Rvalue::Cast(operand, ty) => {
                let value = Self::translate_operand(
                    builder,
                    operand,
                    locals,
                    local_types,
                    type_definitions,
                )?;
                let dest_ty = translate_type(ty);
                let src_ty = builder.func.dfg.value_type(value);

                Self::cast_value(builder, value, src_ty, dest_ty)
            }

            Rvalue::Len(_place) => {
                // For now, return a placeholder length
                // Full implementation would dereference the place and get length
                // This is currently only used in for-loops over lists
                // where we rewrite to while loops with explicit bounds
                Ok(builder.ins().iconst(cl_types::I64, 0))
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
    fn translate_operand(
        builder: &mut FunctionBuilder,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<Value, String> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                Self::read_place(builder, place, locals, local_types, type_definitions)
            }

            Operand::Constant(constant) => Self::translate_constant(builder, constant),
        }
    }

    /// Translate a constant to a Cranelift value.
    fn translate_constant(
        builder: &mut FunctionBuilder,
        constant: &Constant,
    ) -> Result<Value, String> {
        let cl_type = translate_type(&constant.ty);

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

            Literal::String(_) => {
                // String literals are represented as pointers (I64)
                // For now, return a placeholder null pointer
                Ok(builder.ins().iconst(cl_types::I64, 0))
            }

            Literal::Symbol(_) => {
                // Symbols are represented as I64
                Ok(builder.ins().iconst(cl_types::I64, 0))
            }

            Literal::Regex(_) => Err("Regex constants not supported in codegen".to_string()),
        }
    }

    /// Translate a binary operation.
    fn translate_binop(
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: Value,
        rhs: Value,
    ) -> Result<Value, String> {
        let ty = builder.func.dfg.value_type(lhs);
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
                    // Signed division
                    builder.ins().sdiv(lhs, rhs)
                }
            }
            BinOp::Rem => {
                if is_float {
                    return Err("Floating point remainder not directly supported".to_string());
                } else {
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
    fn translate_unop(
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
                // Bitwise not for integers, logical not for bools
                builder.ins().bnot(val)
            }
            UnOp::Await => {
                return Err("Await not supported in synchronous codegen".to_string());
            }
        };

        Ok(result)
    }

    /// Read a value from a place.
    fn read_place(
        builder: &mut FunctionBuilder,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<Value, String> {
        let var = locals
            .get(&place.local)
            .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;

        let mut value = builder.use_var(*var);

        for proj in &place.projection {
            match proj {
                PlaceElem::Deref => {
                    value = builder.ins().load(cl_types::I64, MemFlags::new(), value, 0);
                }
                PlaceElem::Field(idx) => {
                    let base_type = &local_types[place.local.0];
                    let (offset, field_ty) =
                        layout::field_layout(&base_type.kind, *idx, type_definitions);
                    value = builder.ins().load(field_ty, MemFlags::new(), value, offset);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals
                        .get(local)
                        .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                    let idx_val = builder.use_var(*idx_var);
                    let elem_size = builder.ins().iconst(cl_types::I64, 8);
                    let byte_offset = builder.ins().imul(idx_val, elem_size);
                    let elem_addr = builder.ins().iadd(value, byte_offset);
                    value = builder
                        .ins()
                        .load(cl_types::I64, MemFlags::new(), elem_addr, 0);
                }
            }
        }

        Ok(value)
    }

    /// Cast a value to instances of another type.
    fn cast_value(
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
                // Assume signed extension as Miri defaults to signed ints
                Ok(builder.ins().sextend(to_ty, value))
                // For unsigned extension, use uextend
                // builder.ins().uextend(to_ty, value)
            }
        } else {
            Err(format!(
                "Unsupported implicit cast from {} to {}",
                from_ty, to_ty
            ))
        }
    }

    /// Assign a value to a place.
    fn assign_to_place(
        builder: &mut FunctionBuilder,
        place: &Place,
        value: Value,
        locals: &HashMap<Local, Variable>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), String> {
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
                        addr = builder.ins().load(cl_types::I64, MemFlags::new(), addr, 0);
                    }
                    PlaceElem::Field(idx) => {
                        let base_type = &local_types[place.local.0];
                        let (offset, _) =
                            layout::field_layout(&base_type.kind, *idx, type_definitions);
                        addr = builder.ins().iadd_imm(addr, offset as i64);
                    }
                    PlaceElem::Index(local) => {
                        let idx_var = locals
                            .get(local)
                            .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                        let idx_val = builder.use_var(*idx_var);
                        let elem_size = builder.ins().iconst(cl_types::I64, 8);
                        let byte_offset = builder.ins().imul(idx_val, elem_size);
                        addr = builder.ins().iadd(addr, byte_offset);
                    }
                }
            }

            // Apply the last projection as a store
            match place.projection.last().unwrap() {
                PlaceElem::Deref => {
                    builder.ins().store(MemFlags::new(), value, addr, 0);
                }
                PlaceElem::Field(idx) => {
                    let base_type = &local_types[place.local.0];
                    let (offset, _) = layout::field_layout(&base_type.kind, *idx, type_definitions);
                    builder.ins().store(MemFlags::new(), value, addr, offset);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals
                        .get(local)
                        .ok_or_else(|| format!("Unknown index local: {:?}", local))?;
                    let idx_val = builder.use_var(*idx_var);
                    let elem_size = builder.ins().iconst(cl_types::I64, 8);
                    let byte_offset = builder.ins().imul(idx_val, elem_size);
                    let elem_addr = builder.ins().iadd(addr, byte_offset);
                    builder.ins().store(MemFlags::new(), value, elem_addr, 0);
                }
            }
        }
        Ok(())
    }

    /// Translate a terminator.
    fn translate_terminator(
        builder: &mut FunctionBuilder,
        terminator: &Terminator,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        local_types: &[Type],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), String> {
        match &terminator.kind {
            TerminatorKind::Return => {
                // Return the value in local 0 (return place)
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
            }

            TerminatorKind::Goto { target } => {
                let target_block = blocks[target];
                builder.ins().jump(target_block, &[]);
            }

            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => {
                let disc_val =
                    Self::translate_operand(builder, discr, locals, local_types, type_definitions)?;

                let disc_ty = builder.func.dfg.value_type(disc_val);

                if targets.len() == 1 {
                    // Simple if-then-else pattern
                    let (value, target) = &targets[0];
                    let then_block = blocks[target];
                    let else_block = blocks[otherwise];

                    // Compare discriminant with target value
                    let cmp_val = builder.ins().iconst(disc_ty, value.value() as i64);
                    let cond = builder.ins().icmp(IntCC::Equal, disc_val, cmp_val);
                    builder.ins().brif(cond, then_block, &[], else_block, &[]);
                } else {
                    // Multi-way branch using a chain of conditionals
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
                }
            }

            TerminatorKind::Call {
                func,
                args: _,
                destination,
                target,
            } => {
                // Handle function calls
                // For now we handle built-in functions and skip others

                // Get function name from operand
                let func_name = match func {
                    Operand::Constant(c) => match &c.literal {
                        Literal::Symbol(name) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };

                // Get destination variable and its type
                let dest_var = locals
                    .get(&destination.local)
                    .ok_or_else(|| format!("Unknown destination local: {:?}", destination.local))?;

                // Get the Cranelift type for this destination from the body
                let dest_ty = &body.local_decls[destination.local.0].ty;
                let cl_ty = translate_type(dest_ty);

                // Create appropriate zero value for the destination type
                let default_val = if cl_ty.is_int() {
                    // Handle all integer types (I8, I16, I32, I64, I128)
                    builder.ins().iconst(cl_ty, 0)
                } else if cl_ty == cl_types::F64 {
                    builder.ins().f64const(0.0)
                } else if cl_ty == cl_types::F32 {
                    builder.ins().f32const(0.0)
                } else {
                    // Default to I64 for unknown types
                    builder.ins().iconst(cl_types::I64, 0)
                };

                match func_name.as_deref() {
                    Some("print") => {
                        // print is a no-op in compiled code (side effect only in interpreter)
                        // Just assign unit value to destination and continue
                        builder.def_var(*dest_var, default_val);

                        // Jump to continuation block
                        if let Some(t) = target {
                            let target_block = blocks[t];
                            builder.ins().jump(target_block, &[]);
                        }
                    }
                    _ => {
                        // For other function calls, we currently don't support them in codegen
                        // This would require module-level function references
                        // For now, assign unit value and continue
                        builder.def_var(*dest_var, default_val);

                        if let Some(t) = target {
                            let target_block = blocks[t];
                            builder.ins().jump(target_block, &[]);
                        }
                    }
                }
            }

            TerminatorKind::Unreachable => {
                builder.ins().trap(TrapCode::user(0).unwrap());
            }

            TerminatorKind::GpuLaunch { .. } => {
                return Err("GPU launches not supported in CPU backend".to_string());
            }
        }

        Ok(())
    }

    /// Helper to call libc malloc.
    fn call_libc_malloc(builder: &mut FunctionBuilder, _size: Value) -> Result<Value, String> {
        let _sig = builder.import_signature(Signature {
            params: vec![AbiParam::new(cl_types::I64)],
            returns: vec![AbiParam::new(cl_types::I64)],
            call_conv: CallConv::SystemV, // Default to SystemV for now, should be target dependent
        });

        Ok(builder.ins().iconst(cl_types::I64, 0)) // STUB for compilation check
    }

    /// Helper to call libc free.
    fn call_libc_free(_builder: &mut FunctionBuilder, _ptr: Value) -> Result<(), String> {
        // STUB
        // let sig = ...
        // builder.ins().call(...)
        Ok(())
    }
}
