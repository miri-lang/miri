// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::{Type, TypeKind};
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{
    BasicBlock, BinOp, Body, Constant, Local, Operand, Place, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind, UnOp,
};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, Block, Function, InstBuilder, Signature, TrapCode, Value};
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
}

impl FunctionTranslator {
    /// Create a new function translator.
    pub fn new(isa: &Arc<dyn TargetIsa>, body: &Body) -> Self {
        let func = Function::new();
        let builder_ctx = FunctionBuilderContext::new();

        // Cache local types
        let local_types = body.local_decls.iter().map(|d| d.ty.clone()).collect();

        Self {
            func,
            builder_ctx,
            call_conv: isa.default_call_conv(),
            local_types,
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
            let var = Variable::from_u32(idx as u32);

            builder.declare_var(var, cl_type);
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
                Self::translate_statement(&mut builder, stmt, &locals, &self.local_types)?;
            }

            // Translate the terminator
            if let Some(ref terminator) = block_data.terminator {
                Self::translate_terminator(&mut builder, terminator, body, &locals, &blocks)?;
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
        _local_types: &[Type],
    ) -> Result<(), String> {
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                let value = Self::translate_rvalue(builder, rvalue, locals)?;
                Self::assign_to_place(builder, place, value, locals)?;
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
    ) -> Result<Value, String> {
        match rvalue {
            Rvalue::Use(operand) => Self::translate_operand(builder, operand, locals),

            Rvalue::BinaryOp(op, lhs, rhs) => {
                let lhs_val = Self::translate_operand(builder, lhs, locals)?;
                let rhs_val = Self::translate_operand(builder, rhs, locals)?;
                Self::translate_binop(builder, *op, lhs_val, rhs_val)
            }

            Rvalue::UnaryOp(op, operand) => {
                let val = Self::translate_operand(builder, operand, locals)?;
                Self::translate_unop(builder, *op, val)
            }

            Rvalue::Ref(_place) => {
                // TODO: Implement reference creation
                Err("Reference creation not yet implemented".to_string())
            }

            Rvalue::Aggregate(_kind, _operands) => {
                // TODO: Implement aggregate construction
                Err("Aggregate construction not yet implemented".to_string())
            }

            Rvalue::Cast(_operand, _ty) => {
                // TODO: Implement casts
                Err("Cast operations not yet implemented".to_string())
            }

            Rvalue::Len(_place) => {
                // TODO: Implement length
                Err("Length operations not yet implemented".to_string())
            }

            Rvalue::GpuIntrinsic(_intrinsic) => {
                Err("GPU intrinsics not supported in CPU backend".to_string())
            }
        }
    }

    /// Translate an operand to a Cranelift value.
    fn translate_operand(
        builder: &mut FunctionBuilder,
        operand: &Operand,
        locals: &HashMap<Local, Variable>,
    ) -> Result<Value, String> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => Self::read_place(builder, place, locals),

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

            Literal::Float(float_lit) => match float_lit {
                FloatLiteral::F32(bits) => {
                    let val = f32::from_bits(*bits);
                    Ok(builder.ins().f32const(val))
                }
                FloatLiteral::F64(bits) => {
                    let val = f64::from_bits(*bits);
                    Ok(builder.ins().f64const(val))
                }
            },

            Literal::Boolean(val) => {
                let int_val = if *val { 1i64 } else { 0i64 };
                Ok(builder.ins().iconst(cl_types::I8, int_val))
            }

            Literal::None => {
                // Unit/None is represented as 0
                Ok(builder.ins().iconst(cl_types::I8, 0))
            }

            Literal::String(_) => {
                // TODO: Implement string constants
                Err("String constants not yet implemented".to_string())
            }

            Literal::Symbol(_) => {
                // TODO: Implement symbol constants
                Err("Symbol constants not yet implemented".to_string())
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
    ) -> Result<Value, String> {
        let var = locals
            .get(&place.local)
            .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;

        let value = builder.use_var(*var);

        // Handle projections (field access, indexing, etc.)
        if !place.projection.is_empty() {
            // TODO: Implement projections
            return Err("Place projections not yet implemented".to_string());
        }

        Ok(value)
    }

    /// Assign a value to a place.
    fn assign_to_place(
        builder: &mut FunctionBuilder,
        place: &Place,
        value: Value,
        locals: &HashMap<Local, Variable>,
    ) -> Result<(), String> {
        if !place.projection.is_empty() {
            return Err("Place projections not yet implemented".to_string());
        }

        let var = locals
            .get(&place.local)
            .ok_or_else(|| format!("Unknown local: {:?}", place.local))?;

        builder.def_var(*var, value);
        Ok(())
    }

    /// Translate a terminator.
    fn translate_terminator(
        builder: &mut FunctionBuilder,
        terminator: &Terminator,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
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
                let disc_val = Self::translate_operand(builder, discr, locals)?;

                if targets.len() == 1 {
                    // Simple if-then-else pattern
                    let (value, target) = &targets[0];
                    let then_block = blocks[target];
                    let else_block = blocks[otherwise];

                    // Compare discriminant with target value
                    let cmp_val = builder.ins().iconst(cl_types::I64, *value as i64);
                    let cond = builder.ins().icmp(IntCC::Equal, disc_val, cmp_val);
                    builder.ins().brif(cond, then_block, &[], else_block, &[]);
                } else {
                    // Multi-way branch using a chain of conditionals
                    let mut remaining_targets: Vec<_> = targets.iter().collect();
                    let otherwise_block = blocks[otherwise];

                    while let Some((value, target)) = remaining_targets.pop() {
                        let target_block = blocks[target];
                        let cmp_val = builder.ins().iconst(cl_types::I64, *value as i64);
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
                func: _,
                args: _,
                destination: _,
                target: _,
            } => {
                // TODO: Implement function calls
                return Err("Function calls not yet implemented".to_string());
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
}
