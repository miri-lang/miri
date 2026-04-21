use crate::ast::expression::ExpressionKind;
use crate::ast::literal::Literal;
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{
    AggregateKind, BasicBlock, Body, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
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
                let ptr = Self::read_place(builder, ctx, place, locals, type_ctx)?;

                // Guard: skip if pointer is null (uninitialized local)
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
            }
            StatementKind::DecRef(place) => {
                // Uniform RC decrement for all heap types.
                // When RC reaches zero, call type-appropriate cleanup.
                // Resolve the actual field type when place has projections (e.g. h.data).
                let place_kind_cow = Self::resolve_projected_type_kind(place, type_ctx);
                let ptr = Self::read_place(builder, ctx, place, locals, type_ctx)?;

                // Guard: skip if pointer is null (uninitialized local)
                let null = builder.ins().iconst(ptr_type, 0);
                let is_null = builder.ins().icmp(IntCC::Equal, ptr, null);
                let rc_block = builder.create_block();
                let merge_block = builder.create_block();
                builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

                builder.switch_to_block(rc_block);
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);

                let is_immortal = builder.ins().icmp_imm(IntCC::SignedLessThan, rc, 0);
                let dec_block = builder.create_block();
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
                Self::emit_type_drop(builder, ctx, &place_kind_cow, ptr, header_ptr, type_ctx)?;
                builder.ins().jump(merge_block, &[]);

                builder.seal_block(rc_block);
                builder.seal_block(dec_block);
                builder.seal_block(free_block);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
            }
            StatementKind::Dealloc(place) => {
                // Unconditional cleanup — the caller has already determined
                // this value needs freeing (e.g., unique owner going out of scope).
                // Guard against null (uninitialized locals).
                let place_kind_cow = Self::resolve_projected_type_kind(place, type_ctx);
                let ptr = Self::read_place(builder, ctx, place, locals, type_ctx)?;

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
            }
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                let mut value = Self::translate_rvalue(builder, ctx, rvalue, locals, type_ctx)?;

                // Handle implicit casts (e.g. float -> f32, u8 -> u32)
                let dest_ty = &type_ctx.local_types[place.local.0];
                let dest_cl_ty = translate_type(dest_ty, ptr_type);
                let val_ty = builder.func.dfg.value_type(value);

                if dest_cl_ty != val_ty {
                    let is_unsigned = Self::is_unsigned_type_kind(&dest_ty.kind);
                    value = Self::cast_value_with_sign(
                        builder,
                        value,
                        val_ty,
                        dest_cl_ty,
                        is_unsigned,
                    )?;
                }

                Self::assign_to_place(builder, ctx, place, value, locals, type_ctx)?;

                // After constructing an empty Set<T>(), set elem_drop_fn from the
                // destination type. translate_rvalue has no operands to inspect for
                // this path, so we derive the element type from the assignment target.
                if let Rvalue::Aggregate(AggregateKind::Set, ops) = rvalue {
                    if ops.is_empty() {
                        if let Some(elem_expr) = FunctionTranslator::set_elem_expr(&dest_ty.kind) {
                            if let ExpressionKind::Type(elem_ty, _) = &elem_expr.node {
                                FunctionTranslator::emit_set_drop_fn_for_elem_kind(
                                    builder,
                                    ctx,
                                    &elem_ty.kind,
                                    value,
                                    ptr_type,
                                )?;
                            }
                        }
                    }
                }
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
                // Determine whether this is a direct (named) or indirect (function-pointer) call.
                let func_name = match func {
                    Operand::Constant(c) => match &c.literal {
                        Literal::Identifier(name) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };

                // Translate call arguments and build the call signature.
                //
                // If the function was already pre-declared (user-defined functions
                // are pre-declared with correct MIR-derived signatures), we look up
                // the existing declaration and cast argument values to match the
                // declared parameter types.  This avoids signature mismatches when
                // the DFG value type differs from the declared type (e.g. u8 stored
                // as I64 in the DFG vs I8 in the declaration).
                let mut arg_values = Vec::new();

                // Look up any pre-existing declaration for this function.
                let predeclared_sig = func_name.as_deref().and_then(|name| {
                    use cranelift_module::FuncOrDataId;
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
                });

                // Runtime collection functions use pointer-sized values for element
                // arguments to maintain a consistent FFI signature regardless of the
                // element type (bool/i8, int/i64, etc.).
                let widen_value_args = func_name
                    .as_deref()
                    .is_some_and(|n| n == rt::LIST_PUSH || n == rt::LIST_INSERT);

                let mut sig = Signature::new(builder.func.signature.call_conv);

                for (i, arg) in args.iter().enumerate() {
                    let val = Self::translate_operand(builder, ctx, arg, locals, type_ctx)?;
                    let val_ty = builder.func.dfg.value_type(val);

                    // For collection runtime calls, widen non-pointer element values
                    // (skip arg 0 which is the collection pointer)
                    let val = if widen_value_args && i > 0 && val_ty.bytes() < ptr_type.bytes() {
                        builder.ins().sextend(ptr_type, val)
                    } else {
                        val
                    };

                    // Cast argument to match pre-declared parameter type if needed.
                    let val = if let Some(ref pre_sig) = predeclared_sig {
                        if i < pre_sig.params.len() {
                            let expected_ty = pre_sig.params[i].value_type;
                            let actual_ty = builder.func.dfg.value_type(val);
                            if actual_ty != expected_ty {
                                if actual_ty.bytes() > expected_ty.bytes() {
                                    builder.ins().ireduce(expected_ty, val)
                                } else {
                                    builder.ins().sextend(expected_ty, val)
                                }
                            } else {
                                val
                            }
                        } else {
                            val
                        }
                    } else {
                        val
                    };

                    arg_values.push(val);
                    sig.params
                        .push(AbiParam::new(builder.func.dfg.value_type(val)));
                }

                let dest_ty = &body.local_decls[destination.local.0].ty;
                let cl_dest_ty = translate_type(dest_ty, ptr_type);
                if dest_ty.kind != TypeKind::Void {
                    sig.returns.push(AbiParam::new(cl_dest_ty));
                }

                // Pre-compute flags before consuming func_name in the match.
                let is_list_from_managed =
                    func_name.as_deref() == Some(rt::LIST_NEW_FROM_MANAGED_ARRAY);
                // Empty List<T>() constructor: elem_drop_fn must be set from the
                // destination type because there are no operands to inspect.
                let is_list_new_empty = func_name.as_deref() == Some(rt::LIST_NEW);

                if let Some(func_name) = func_name {
                    // Direct call to a named symbol.
                    let func_id = ctx
                        .module
                        .declare_function(&func_name, Linkage::Import, &sig)
                        .map_err(|e| format!("Failed to declare function {}: {}", func_name, e))?;
                    let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
                    let call = builder.ins().call(local_func, &arg_values);

                    let maybe_result = if dest_ty.kind != TypeKind::Void {
                        let result = builder.inst_results(call)[0];
                        let dest_var = locals.get(&destination.local).ok_or_else(|| {
                            format!("Unknown call destination local: {:?}", destination.local)
                        })?;
                        builder.def_var(*dest_var, result);
                        Some(result)
                    } else {
                        None
                    };

                    // After miri_rt_list_new_from_managed_array, that runtime sets
                    // elem_drop_fn = miri_rt_list_decref_element for all managed types.
                    // Override with __decref_TypeName when elements are user-defined classes.
                    if is_list_from_managed {
                        if let (Some(list_ptr), Some(array_arg)) = (maybe_result, args.first()) {
                            let array_kind = match array_arg {
                                Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => {
                                    Some(&type_ctx.local_types[p.local.0].kind)
                                }
                                _ => None,
                            };
                            if let Some(array_kind) = array_kind {
                                if let Some(elem_expr) =
                                    FunctionTranslator::collection_elem_expr(array_kind)
                                {
                                    if let ExpressionKind::Type(inner_ty, _) = &elem_expr.node {
                                        // The runtime unconditionally sets elem_drop_fn =
                                        // miri_rt_list_decref_element for all managed types.
                                        // Override here for non-List managed element types so that
                                        // clear/remove_at call the correct decref function.
                                        let decref_addr_opt: Option<
                                            Result<cranelift_codegen::ir::Value, String>,
                                        > = match &inner_ty.kind {
                                            TypeKind::Array(_, _) => {
                                                Some(FunctionTranslator::get_rt_array_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Set(_) => {
                                                Some(FunctionTranslator::get_rt_set_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Map(_, _) => {
                                                Some(FunctionTranslator::get_rt_map_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Custom(type_name, Some(_))
                                                if BuiltinCollectionKind::from_name(type_name)
                                                    == Some(BuiltinCollectionKind::Array) =>
                                            {
                                                Some(FunctionTranslator::get_rt_array_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Custom(type_name, Some(_))
                                                if BuiltinCollectionKind::from_name(type_name)
                                                    == Some(BuiltinCollectionKind::Set) =>
                                            {
                                                Some(FunctionTranslator::get_rt_set_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Custom(type_name, Some(_))
                                                if BuiltinCollectionKind::from_name(type_name)
                                                    == Some(BuiltinCollectionKind::Map) =>
                                            {
                                                Some(FunctionTranslator::get_rt_map_decref_element_addr(
                                                    builder, ctx, ptr_type,
                                                ))
                                            }
                                            TypeKind::Custom(type_name, _)
                                                if BuiltinCollectionKind::from_name(type_name)
                                                    .is_none() =>
                                            {
                                                Some(FunctionTranslator::get_custom_decref_thunk_addr(
                                                    builder, ctx, type_name, ptr_type,
                                                ))
                                            }
                                            _ => None,
                                        };
                                        if let Some(decref_addr) = decref_addr_opt {
                                            FunctionTranslator::call_rt_list_set_elem_drop_fn(
                                                builder,
                                                ctx,
                                                list_ptr,
                                                decref_addr?,
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // After miri_rt_list_new (empty List<T>() constructor), set elem_drop_fn
                    // from the destination type. translate_rvalue has no operands to inspect
                    // for this path, so we must derive the element type from the call site type.
                    if is_list_new_empty {
                        if let Some(list_ptr) = maybe_result {
                            if let Some(elem_expr) =
                                FunctionTranslator::collection_elem_expr(&dest_ty.kind)
                            {
                                if let ExpressionKind::Type(elem_ty, _) = &elem_expr.node {
                                    FunctionTranslator::emit_list_drop_fn_for_elem_kind(
                                        builder,
                                        ctx,
                                        &elem_ty.kind,
                                        list_ptr,
                                        ptr_type,
                                    )?;
                                }
                            }
                        }
                    }
                } else {
                    // Indirect call through a closure struct.
                    // Layout: payload_ptr[0] = fn_ptr, payload_ptr[ptr_size..] = captures.
                    let closure_ptr =
                        Self::translate_operand(builder, ctx, func, locals, type_ctx)?;

                    // Load fn_ptr from closure struct (first word of payload).
                    let fn_ptr = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), closure_ptr, 0);

                    // Prepend env_ptr (= closure_ptr) to the argument list.
                    let mut full_args = vec![closure_ptr];
                    full_args.extend_from_slice(&arg_values);

                    // Prepend env_ptr to the signature.
                    let mut full_sig = Signature::new(builder.func.signature.call_conv);
                    full_sig.params.push(AbiParam::new(ptr_type)); // env_ptr
                    full_sig.params.extend(sig.params);
                    full_sig.returns.extend(sig.returns);

                    let sig_ref = builder.import_signature(full_sig);
                    let call = builder.ins().call_indirect(sig_ref, fn_ptr, &full_args);

                    if dest_ty.kind != TypeKind::Void {
                        let result = builder.inst_results(call)[0];
                        let dest_var = locals.get(&destination.local).ok_or_else(|| {
                            format!("Unknown call destination local: {:?}", destination.local)
                        })?;
                        builder.def_var(*dest_var, result);
                    }
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

            TerminatorKind::VirtualCall {
                vtable_slot,
                args,
                destination,
                target,
            } => {
                // args[0] is the receiver. Load vtable ptr from receiver[0], then
                // load fn_ptr from vtable[vtable_slot * ptr_size], then call_indirect.
                debug_assert!(
                    !args.is_empty(),
                    "VirtualCall must have at least one arg (receiver)"
                );

                // Translate all arguments
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

                // Load vtable pointer from receiver[0]
                let receiver_ptr = arg_values[0];
                let vtable_ptr = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), receiver_ptr, 0);

                // Load fn_ptr from vtable[slot * ptr_size]
                let slot_offset = (*vtable_slot as i32) * ptr_type.bytes() as i32;
                let fn_ptr = builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), vtable_ptr, slot_offset);

                // Call via call_indirect — receiver is already in arg_values[0]
                let sig_ref = builder.import_signature(sig);
                let call = builder.ins().call_indirect(sig_ref, fn_ptr, &arg_values);

                if dest_ty.kind != TypeKind::Void {
                    let result = builder.inst_results(call)[0];
                    let dest_var = locals.get(&destination.local).ok_or_else(|| {
                        format!("Unknown vcall destination local: {:?}", destination.local)
                    })?;
                    builder.def_var(*dest_var, result);
                }

                if let Some(t) = target {
                    let target_block = blocks[t];
                    builder.ins().jump(target_block, &[]);
                }
            }
        }

        Ok(())
    }

    /// Resolve the `TypeKind` of a place after following its `Field` projections.
    ///
    /// For an unprojected local, returns the local's type kind directly.
    /// For a `Field(i)` projection on a `Custom` type, looks up the field type from
    /// `type_definitions` so that callers like `emit_type_drop` receive the correct
    /// kind (e.g. `List([int])`) instead of the container's kind (e.g. `Custom("Holder")`).
    fn resolve_projected_type_kind(place: &Place, type_ctx: &TypeCtx) -> TypeKind {
        let mut current = type_ctx.local_types[place.local.0].kind.clone();

        for proj in &place.projection {
            match proj {
                PlaceElem::Field(idx) => {
                    current = match &current {
                        TypeKind::Custom(name, _) => {
                            match type_ctx.type_definitions.get(name.as_str()) {
                                Some(crate::type_checker::context::TypeDefinition::Struct(def)) => {
                                    def.fields
                                        .get(*idx)
                                        .map(|(_, ty, _)| ty.kind.clone())
                                        .unwrap_or(TypeKind::Error)
                                }
                                Some(crate::type_checker::context::TypeDefinition::Class(def)) => {
                                    let all_fields =
                                        crate::type_checker::context::collect_class_fields_all(
                                            def,
                                            type_ctx.type_definitions,
                                        );
                                    all_fields
                                        .get(*idx)
                                        .map(|(_, fi)| fi.ty.kind.clone())
                                        .unwrap_or(TypeKind::Error)
                                }
                                _ => TypeKind::Error,
                            }
                        }
                        _ => TypeKind::Error,
                    };
                }
                _ => break,
            }
        }

        current
    }
}
