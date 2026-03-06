use crate::ast::literal::Literal;
use crate::ast::types::TypeKind;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Statement, StatementKind, Terminator, TerminatorKind,
};
use cranelift_codegen::ir::{
    condcodes::IntCC, AbiParam, Block, InstBuilder, MemFlags, Signature, TrapCode,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

impl<'a> FunctionTranslator<'a> {
    /// Translate a MIR statement.
    pub(crate) fn translate_statement(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        stmt: &Statement,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        match &stmt.kind {
            StatementKind::IncRef(place) => {
                // Uniform RC increment for all heap types.
                // All heap values use [RC][payload] layout; ptr points past RC.
                let ptr = Self::read_place(builder, place, locals, type_ctx)?;
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);

                let is_immortal = builder.ins().icmp_imm(IntCC::SignedLessThan, rc, 0);
                let then_block = builder.create_block();
                let merge_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_immortal, merge_block, &[], then_block, &[]);

                builder.switch_to_block(then_block);
                let new_rc = builder.ins().iadd_imm(rc, 1);
                builder.ins().store(MemFlags::new(), new_rc, header_ptr, 0);
                builder.ins().jump(merge_block, &[]);

                builder.seal_block(then_block);
                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
            }
            StatementKind::DecRef(place) => {
                // Uniform RC decrement for all heap types.
                // When RC reaches zero, call type-appropriate cleanup.
                let place_ty = &type_ctx.local_types[place.local.0];
                let ptr = Self::read_place(builder, place, locals, type_ctx)?;
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);

                let is_immortal = builder.ins().icmp_imm(IntCC::SignedLessThan, rc, 0);
                let dec_block = builder.create_block();
                let merge_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_immortal, merge_block, &[], dec_block, &[]);

                builder.switch_to_block(dec_block);
                let new_rc = builder.ins().iadd_imm(rc, -1);
                builder.ins().store(MemFlags::new(), new_rc, header_ptr, 0);

                let zero = builder.ins().iconst(ptr_type, 0);
                let is_zero = builder.ins().icmp(IntCC::Equal, new_rc, zero);

                let free_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_zero, free_block, &[], merge_block, &[]);

                builder.switch_to_block(free_block);
                Self::emit_type_drop(builder, ctx, &place_ty.kind, ptr, header_ptr, type_ctx)?;
                builder.ins().jump(merge_block, &[]);

                builder.seal_block(dec_block);
                builder.seal_block(free_block);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
            }
            StatementKind::Dealloc(place) => {
                // Unconditional cleanup — the caller has already determined
                // this value needs freeing (e.g., unique owner going out of scope).
                let place_ty = &type_ctx.local_types[place.local.0];
                let ptr = Self::read_place(builder, place, locals, type_ctx)?;
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                Self::emit_type_drop(builder, ctx, &place_ty.kind, ptr, header_ptr, type_ctx)?;
            }
            StatementKind::Assign(place, rvalue) => {
                let mut value = Self::translate_rvalue(builder, ctx, rvalue, locals, type_ctx)?;

                // Handle implicit casts (e.g. float -> f32, i8 -> i32)
                let dest_ty = &type_ctx.local_types[place.local.0];
                let dest_cl_ty = translate_type(dest_ty, ptr_type);
                let val_ty = builder.func.dfg.value_type(value);

                if dest_cl_ty != val_ty {
                    value = Self::cast_value(builder, value, val_ty, dest_cl_ty)?;
                }

                Self::assign_to_place(builder, place, value, locals, type_ctx)?;
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
    /// Translate a terminator.
    pub(crate) fn translate_terminator(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        terminator: &Terminator,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        type_ctx: &TypeCtx,
    ) -> Result<(), String> {
        let ptr_type = type_ctx.ptr_type;
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
                let disc_val = Self::translate_operand(builder, ctx, discr, locals, type_ctx)?;

                let disc_ty = builder.func.dfg.value_type(disc_val);

                if targets.is_empty() {
                    // No targets — unconditional jump to otherwise
                    let otherwise_block = blocks[otherwise];
                    builder.ins().jump(otherwise_block, &[]);
                } else if targets.len() == 1 {
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
                args,
                destination,
                target,
            } => {
                // Handle function calls
                let func_name = match func {
                    Operand::Constant(c) => match &c.literal {
                        Literal::Identifier(name) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };

                let func_name =
                    func_name.ok_or_else(|| "Indirect calls not supported".to_string())?;

                // Check if it's a runtime function
                // For now, we assume all reachable calls in MIR are either internal or runtime functions.
                // We'll try to find it in the module imports.
                let mut sig = Signature::new(builder.func.signature.call_conv);
                let mut arg_values = Vec::new();

                for arg in args {
                    let val = Self::translate_operand(builder, ctx, arg, locals, type_ctx)?;
                    arg_values.push(val);
                    sig.params
                        .push(AbiParam::new(builder.func.dfg.value_type(val)));
                }

                let dest_ty = &body.local_decls[destination.local.0].ty;
                let cl_dest_ty = translate_type(dest_ty, ptr_type);
                if dest_ty.kind != TypeKind::Void {
                    sig.returns.push(AbiParam::new(cl_dest_ty));
                }

                let func_id = ctx
                    .module
                    .declare_function(&func_name, Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare function {}: {}", func_name, e))?;
                let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
                let call = builder.ins().call(local_func, &arg_values);

                if dest_ty.kind != TypeKind::Void {
                    let result = builder.inst_results(call)[0];
                    let dest_var = locals.get(&destination.local).ok_or_else(|| {
                        format!("Unknown call destination local: {:?}", destination.local)
                    })?;
                    builder.def_var(*dest_var, result);
                }

                if let Some(t) = target {
                    let target_block = blocks[t];
                    builder.ins().jump(target_block, &[]);
                }
            }

            TerminatorKind::Unreachable => {
                let trap_code = TrapCode::user(1)
                    .ok_or_else(|| "Failed to create user trap code".to_string())?;
                builder.ins().trap(trap_code);
            }

            TerminatorKind::GpuLaunch { .. } => {
                return Err("GPU launches not supported in CPU backend".to_string());
            }
        }

        Ok(())
    }
}
