use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::codegen::cranelift::layout::field_layout;
use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, TypeCtx};
use crate::codegen::cranelift::types::translate_type;
use crate::mir::{AggregateKind, BinOp, Constant, Local, Operand, Rvalue, UnOp};
use crate::type_checker::context::class_needs_vtable;
use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types as cl_types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, TrapCode, Value,
};
use cranelift_codegen::ir::{AbiParam, Signature};
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

                // Layout: [padding][malloc_ptr][RC][payload...]
                //
                // We over-allocate so the payload can be aligned. The RC header
                // lives at (payload - ptr_size), and the real malloc pointer is
                // stored at (payload - 2*ptr_size). This lets free() recover the
                // original pointer even when alignment shifts the payload.
                let header_overhead = builder.ins().iconst(ptr_type, 2 * ptr_size as i64);
                let total_size = builder.ins().iadd(size, align);
                let total_size = builder.ins().iadd(total_size, header_overhead);

                let raw_ptr = Self::call_libc_malloc(builder, ctx, total_size)?;

                // Null-check: trap on OOM
                let null = builder.ins().iconst(ptr_type, 0);
                let is_null = builder.ins().icmp(
                    cranelift_codegen::ir::condcodes::IntCC::Equal,
                    raw_ptr,
                    null,
                );
                let trap_code = TrapCode::user(2).expect("valid user trap code");
                builder.ins().trapnz(is_null, trap_code);

                // Payload starts at align_to(raw_ptr + 2*ptr_size, align)
                let payload_base = builder.ins().iadd(raw_ptr, header_overhead);
                let mask = builder.ins().ineg(align);
                let align_minus_1 = builder.ins().iadd_imm(align, -1);
                let bumped = builder.ins().iadd(payload_base, align_minus_1);
                let ptr = builder.ins().band(bumped, mask);

                // Store the real malloc pointer at (ptr - 2*ptr_size)
                let malloc_slot = builder.ins().iadd_imm(ptr, -(2 * ptr_size as i64));
                builder
                    .ins()
                    .store(MemFlags::new(), raw_ptr, malloc_slot, 0);

                // RC header lives at (ptr - ptr_size), initialize to 1
                let header_ptr = builder.ins().iadd_imm(ptr, -(ptr_size as i64));
                let one = builder.ins().iconst(ptr_type, 1);
                builder.ins().store(MemFlags::new(), one, header_ptr, 0);

                Ok(ptr)
            }
            Rvalue::Use(operand) => {
                Self::translate_operand(builder, ctx, operand, locals, type_ctx)
            }

            Rvalue::BinaryOp(op, lhs, rhs) => {
                // Structural equality: compare field-by-field instead of
                // pointer comparison for tuples and structs.
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    let lhs_kind = Self::operand_type_kind(lhs, type_ctx);

                    let structural_eq_result = match lhs_kind {
                        TypeKind::Tuple(element_exprs) => {
                            let lhs_val =
                                Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                            let rhs_val =
                                Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
                            Some(Self::translate_tuple_equality(
                                builder,
                                ctx,
                                lhs_val,
                                rhs_val,
                                element_exprs,
                                type_ctx,
                            )?)
                        }
                        TypeKind::Custom(name, _) => {
                            if let Some(crate::type_checker::context::TypeDefinition::Struct(def)) =
                                type_ctx.type_definitions.get(name)
                            {
                                let lhs_val =
                                    Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                                let rhs_val =
                                    Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
                                Some(Self::translate_struct_equality(
                                    builder, lhs_val, rhs_val, lhs_kind, def, type_ctx,
                                )?)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(result) = structural_eq_result {
                        return if *op == BinOp::Ne {
                            let one = builder.ins().iconst(cranelift_codegen::ir::types::I8, 1);
                            Ok(builder.ins().bxor(result, one))
                        } else {
                            Ok(result)
                        };
                    }
                }

                let lhs_val = Self::translate_operand(builder, ctx, lhs, locals, type_ctx)?;
                let rhs_val = Self::translate_operand(builder, ctx, rhs, locals, type_ctx)?;
                // Determine signedness from the operand types so we can
                // select unsigned comparison/shift/division when appropriate.
                let is_unsigned = Self::operand_is_unsigned(lhs, type_ctx)
                    || Self::operand_is_unsigned(rhs, type_ctx);
                Self::translate_binop(builder, ctx, *op, lhs_val, rhs_val, is_unsigned)
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

                            // If elements are managed strings, register a drop fn so
                            // that miri_rt_array_free properly DecRefs each element.
                            if let Some(drop_fn_addr) = Self::resolve_elem_drop_fn(
                                builder, ctx, operands, type_ctx, ptr_type,
                            )? {
                                Self::call_rt_array_set_elem_drop_fn(
                                    builder,
                                    ctx,
                                    array_ptr,
                                    drop_fn_addr,
                                )?;
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

                            // If elements are managed, set elem_drop_fn so that mutation
                            // operations (clear, remove_at, remove) properly DecRef them.
                            if let Some(first_op) = operands.first() {
                                match Self::first_operand_kind(first_op, type_ctx) {
                                    Some(TypeKind::String) => {
                                        let drop_fn_addr = Self::get_rt_string_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_list_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            list_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::List(_)) => {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_list_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            list_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::Custom(n, Some(_)))
                                        if BuiltinCollectionKind::from_name(n)
                                            == Some(BuiltinCollectionKind::List) =>
                                    {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_list_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            list_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    _ => {}
                                }
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
                            let key_kind = if !operands.is_empty() {
                                let first_key_kind =
                                    Self::first_operand_kind(&operands[0], type_ctx);
                                if matches!(first_key_kind, Some(TypeKind::String)) {
                                    1i64
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

                            // If values are managed Lists, set the drop function so that
                            // map mutations (remove, clear, set overwrite) properly DecRef them.
                            if operands.len() >= 2 {
                                let val_op = &operands[1];
                                match Self::first_operand_kind(val_op, type_ctx) {
                                    Some(TypeKind::String) => {
                                        let drop_fn_addr = Self::get_rt_string_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_map_set_val_drop_fn(
                                            builder,
                                            ctx,
                                            map_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::List(_)) => {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_map_set_val_drop_fn(
                                            builder,
                                            ctx,
                                            map_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::Custom(n, Some(_)))
                                        if BuiltinCollectionKind::from_name(n)
                                            == Some(BuiltinCollectionKind::List) =>
                                    {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_map_set_val_drop_fn(
                                            builder,
                                            ctx,
                                            map_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    _ => {}
                                }
                            }

                            // If keys are strings (key_kind == 1), register the string decref
                            // callback so that remove/clear/free properly DecRef string keys.
                            if key_kind == 1 {
                                let drop_fn_addr = Self::get_rt_string_decref_element_addr(
                                    builder, ctx, ptr_type,
                                )?;
                                Self::call_rt_map_set_key_drop_fn(
                                    builder,
                                    ctx,
                                    map_ptr,
                                    drop_fn_addr,
                                )?;
                            }

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

                            // If elements are managed, register a drop fn so that
                            // remove/clear/free properly DecRef removed elements.
                            if let Some(first_op) = operands.first() {
                                match Self::first_operand_kind(first_op, type_ctx) {
                                    Some(TypeKind::String) => {
                                        let drop_fn_addr = Self::get_rt_string_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_set_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            set_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::List(_)) => {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_set_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            set_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    Some(TypeKind::Custom(n, Some(_)))
                                        if BuiltinCollectionKind::from_name(n)
                                            == Some(BuiltinCollectionKind::List) =>
                                    {
                                        let drop_fn_addr = Self::get_rt_list_decref_element_addr(
                                            builder, ctx, ptr_type,
                                        )?;
                                        Self::call_rt_set_set_elem_drop_fn(
                                            builder,
                                            ctx,
                                            set_ptr,
                                            drop_fn_addr,
                                        )?;
                                    }
                                    _ => {}
                                }
                            }

                            Ok(set_ptr)
                        }
                        _ => unreachable!(),
                    }
                } else {
                    // Non-collection aggregates (Tuple, Struct, Class, Enum, etc.)
                    // Empty operands normally mean a zero-sized value (null pointer).
                    // EXCEPTION: vtable-bearing classes must still be heap-allocated so
                    // the vtable pointer slot at payload[0] can be written.
                    let needs_vtable_alloc = if let AggregateKind::Class(ty) = kind {
                        if let TypeKind::Custom(class_name, _) = &ty.kind {
                            class_needs_vtable(class_name, type_ctx.type_definitions)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if operands.is_empty() && !needs_vtable_alloc {
                        return Ok(builder.ins().iconst(ptr_type, 0));
                    }

                    let is_tuple = matches!(kind, AggregateKind::Tuple);

                    // Single-element aggregates can be returned directly UNLESS they need
                    // pointer-based layout (like enums or structs expected as pointers).
                    // Tuples always need full allocation so methods like length() work.
                    let needs_pointer_layout = matches!(
                        kind,
                        AggregateKind::Struct(_)
                            | AggregateKind::Class(_)
                            | AggregateKind::Enum(_, _)
                            | AggregateKind::Tuple
                            | AggregateKind::Option
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

                    // For tuples, prepend a length field (ptr_size) so runtime methods work.
                    // For vtable-bearing classes, prepend a vtable pointer slot (ptr_size).
                    // Layout for classes: [malloc_ptr][RC][vtable_ptr?][field0][field1]...
                    let tuple_header = if is_tuple { ptr_size as u32 } else { 0 };

                    // Determine if this class needs a vtable pointer at offset 0.
                    let vtable_header = if let AggregateKind::Class(ty) = kind {
                        if let TypeKind::Custom(class_name, _) = &ty.kind {
                            if class_needs_vtable(class_name, type_ctx.type_definitions) {
                                Some(class_name.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let vtable_header_size = if vtable_header.is_some() {
                        ptr_size as u32
                    } else {
                        0
                    };

                    // Compute field offsets with proper alignment
                    let mut current_offset: u32 = tuple_header + vtable_header_size;
                    let mut field_offsets = Vec::new();
                    let mut max_align: u32 = if is_tuple { ptr_size as u32 } else { 1 };

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

                    // Heap-allocate with [malloc_ptr][RC][payload] header.
                    // malloc_ptr is stored so free() can recover the original pointer.
                    let header_size = 2 * ptr_size as u32;
                    let alloc_size = builder
                        .ins()
                        .iconst(ptr_type, (total_size + header_size) as i64);
                    let raw_ptr = Self::call_libc_malloc(builder, ctx, alloc_size)?;

                    // Null-check: trap on OOM
                    let null = builder.ins().iconst(ptr_type, 0);
                    let is_null = builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        raw_ptr,
                        null,
                    );
                    let trap_code = TrapCode::user(2).expect("valid user trap code");
                    builder.ins().trapnz(is_null, trap_code);

                    // Store real malloc pointer at offset 0
                    builder.ins().store(MemFlags::new(), raw_ptr, raw_ptr, 0);

                    // Store RC = 1 at offset ptr_size
                    let one = builder.ins().iconst(ptr_type, 1);
                    builder.ins().store(MemFlags::new(), one, raw_ptr, ptr_size);

                    let payload_ptr = builder.ins().iadd_imm(raw_ptr, header_size as i64);

                    // For tuples, store element count at offset 0 of payload
                    if is_tuple {
                        let count = builder.ins().iconst(ptr_type, translated.len() as i64);
                        builder.ins().store(MemFlags::new(), count, payload_ptr, 0);
                    }

                    // For vtable-bearing classes, store vtable pointer at offset 0 of payload.
                    if let Some(ref class_name) = vtable_header {
                        use cranelift_module::Module;
                        let mut vtable_sym = String::with_capacity(9 + class_name.len());
                        vtable_sym.push_str("__vtable_");
                        vtable_sym.push_str(class_name);
                        let vtable_data_id = ctx
                            .module
                            .declare_data(
                                &vtable_sym,
                                cranelift_module::Linkage::Import,
                                false,
                                false,
                            )
                            .map_err(|e| {
                                format!("Failed to declare vtable {}: {}", vtable_sym, e)
                            })?;
                        let gv = ctx
                            .module
                            .declare_data_in_func(vtable_data_id, builder.func);
                        let vtable_ptr = builder.ins().global_value(ptr_type, gv);
                        builder
                            .ins()
                            .store(MemFlags::new(), vtable_ptr, payload_ptr, 0);
                    }

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
                let is_unsigned = Self::is_unsigned_type_kind(&ty.kind);

                Self::cast_value_with_sign(builder, value, src_ty, dest_ty, is_unsigned)
            }

            Rvalue::Len(place) => {
                let ty = type_ctx.local_types[place.local.0];

                // Determine if this is a tuple type (including Custom("Tuple", ...))
                let is_tuple_type = matches!(&ty.kind, TypeKind::Tuple(_))
                    || matches!(&ty.kind, TypeKind::Custom(name, _) if name == "Tuple");

                let len_offset = if Self::is_collection_type(&ty.kind) {
                    // MiriArray.elem_count, MiriList.len, MiriSet.len at offset ptr_size.
                    // MiriMap.len at offset 3*ptr_size (after states, keys, values).
                    Some(if Self::is_map_type(&ty.kind) {
                        ptr_size * 3
                    } else {
                        ptr_size
                    })
                } else if matches!(&ty.kind, TypeKind::String) {
                    // MiriString layout past RC: [DataPtr][Len][Cap]
                    Some(ptr_size)
                } else if is_tuple_type {
                    // Tuple layout: [elem_count][field0][field1]...
                    // Count is stored at offset 0 of payload
                    Some(0)
                } else {
                    None
                };

                if let Some(offset) = len_offset {
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
    ) -> Result<Value, String> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let cl_type = translate_type(&constant.ty, ptr_type);

        match &constant.literal {
            Literal::Integer(int_lit) => {
                match int_lit {
                    IntegerLiteral::I128(v) => {
                        // i128 must be built from two 64-bit halves to avoid truncation
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
                    _ => {
                        let val = match int_lit {
                            IntegerLiteral::I8(v) => *v as i64,
                            IntegerLiteral::I16(v) => *v as i64,
                            IntegerLiteral::I32(v) => *v as i64,
                            IntegerLiteral::I64(v) => *v,
                            IntegerLiteral::U8(v) => *v as i64,
                            IntegerLiteral::U16(v) => *v as i64,
                            IntegerLiteral::U32(v) => *v as i64,
                            IntegerLiteral::U64(v) => *v as i64,
                            IntegerLiteral::I128(_) | IntegerLiteral::U128(_) => unreachable!(),
                        };
                        Ok(builder.ins().iconst(cl_type, val))
                    }
                }
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
                    .map_err(|e| format!("Error declaring string struct: {}", e))?;
                let struct_gv = ctx.module.declare_data_in_func(struct_id, builder.func);
                let struct_addr = builder.ins().symbol_value(ptr_type, struct_gv);

                // Return pointer past RC header (at offset ptr_size) — a valid *const MiriString
                Ok(builder.ins().iadd_imm(struct_addr, ptr_size as i64))
            }

            Literal::Identifier(name) => {
                // For function-typed identifiers (lambdas, named function references),
                // emit a function pointer using func_addr. Build the Cranelift
                // signature from the FunctionTypeData carried in the constant's type.
                if let TypeKind::Function(func_data) = &constant.ty.kind {
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
                        .map_err(|e| format!("Error declaring function {}: {}", name, e))?;
                    let func_ref = ctx.module.declare_func_in_func(func_id, builder.func);
                    Ok(builder.ins().func_addr(ptr_type, func_ref))
                } else {
                    // Non-function identifiers — emit a null pointer (placeholder).
                    Ok(builder.ins().iconst(ptr_type, 0))
                }
            }

            Literal::Regex(_) => Err("Regex constants not supported in codegen".to_string()),
        }
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
    ) -> Result<Value, String> {
        use cranelift_codegen::ir::condcodes::IntCC;
        use cranelift_codegen::ir::TrapCode;
        use cranelift_module::Module;

        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        // Translate captured values.
        let capture_vals: Vec<Value> = capture_ops
            .iter()
            .map(|op| Self::translate_operand(builder, ctx, op, locals, type_ctx))
            .collect::<Result<_, _>>()?;

        // Compute total allocation size:
        //   2 * ptr_size  (header: malloc_ptr + RC)
        // + ptr_size       (fn_ptr slot)
        // + ptr_size * N   (one ptr-size slot per capture)
        let n_captures = capture_vals.len();
        let total_size = (2 + 1 + n_captures as i64) * ptr_size;
        let size_val = builder.ins().iconst(ptr_type, total_size);

        // Allocate raw memory.
        let raw_ptr = Self::call_libc_malloc(builder, ctx, size_val)?;

        // Trap on OOM.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(IntCC::Equal, raw_ptr, null);
        let trap_code = TrapCode::user(2).expect("valid user trap code");
        builder.ins().trapnz(is_null, trap_code);

        // Store malloc_ptr at offset 0 (so free() can recover the original pointer).
        builder.ins().store(MemFlags::new(), raw_ptr, raw_ptr, 0);

        // Store RC = 1 at offset ptr_size.
        let one = builder.ins().iconst(ptr_type, 1);
        builder
            .ins()
            .store(MemFlags::new(), one, raw_ptr, ptr_size as i32);

        // payload_ptr = raw_ptr + 2*ptr_size
        let payload_ptr = builder.ins().iadd_imm(raw_ptr, 2 * ptr_size);

        // Build the lambda's Cranelift signature (env_ptr first, then user params).
        let call_conv = builder.func.signature.call_conv;
        let mut sig = cranelift_codegen::ir::Signature::new(call_conv);
        // env_ptr is always the first parameter.
        sig.params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        if let crate::ast::types::TypeKind::Function(func_data) = &fn_type.kind {
            use crate::ast::expression::ExpressionKind;
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
                    use crate::ast::types::TypeKind;
                    if ret_type.kind != TypeKind::Void {
                        sig.returns
                            .push(cranelift_codegen::ir::AbiParam::new(translate_type(
                                ret_type, ptr_type,
                            )));
                    }
                }
            }
        }

        // Declare the lambda function and get its address.
        let func_id = ctx
            .module
            .declare_function(lambda_name, cranelift_module::Linkage::Import, &sig)
            .map_err(|e| format!("Error declaring closure fn {}: {}", lambda_name, e))?;
        let func_ref = ctx.module.declare_func_in_func(func_id, builder.func);
        let fn_ptr = builder.ins().func_addr(ptr_type, func_ref);

        // Store fn_ptr at payload[0] (offset 0 from payload_ptr).
        builder.ins().store(MemFlags::new(), fn_ptr, payload_ptr, 0);

        // Store each captured value in its slot (widened to ptr_size).
        for (i, val) in capture_vals.into_iter().enumerate() {
            let val_ty = builder.func.dfg.value_type(val);
            let widened =
                if val_ty != ptr_type && val_ty.is_int() && val_ty.bits() < ptr_type.bits() {
                    builder.ins().sextend(ptr_type, val)
                } else {
                    val
                };
            let offset = (1 + i as i32) * ptr_size as i32;
            builder
                .ins()
                .store(MemFlags::new(), widened, payload_ptr, offset);
        }

        Ok(payload_ptr)
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

    /// Returns the address of the appropriate element-drop function for an Array whose
    /// first element is `operands[0]`, or `None` if elements are unmanaged.
    ///
    /// Only `String` is handled here.  List/Custom elements are intentionally excluded:
    /// `[...]` array literals inside `List([...])` constructors are compiler-generated
    /// temporaries whose elements are already owned by the outer List.  Setting
    /// `elem_drop_fn` on those temporaries would cause a double-free.  Support for
    /// `Array<List<T>>` and other managed-element arrays is a follow-up task.
    fn resolve_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        operands: &[Operand],
        type_ctx: &TypeCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Option<Value>, String> {
        let first_op = match operands.first() {
            Some(op) => op,
            None => return Ok(None),
        };
        let kind = match Self::first_operand_kind(first_op, type_ctx) {
            Some(k) => k,
            None => return Ok(None),
        };
        match kind {
            TypeKind::String => {
                let addr = Self::get_rt_string_decref_element_addr(builder, ctx, ptr_type)?;
                Ok(Some(addr))
            }
            _ => Ok(None),
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
    ) -> Result<Value, String> {
        let lhs_ty = builder.func.dfg.value_type(lhs);
        let rhs_ty = builder.func.dfg.value_type(rhs);

        // Ensure both operands have the same type by widening the smaller one.
        let (lhs, rhs, ty) = if lhs_ty != rhs_ty && !lhs_ty.is_float() && !rhs_ty.is_float() {
            // Integer widths differ — extend the narrower operand.
            if lhs_ty.bits() > rhs_ty.bits() {
                let rhs = if is_unsigned {
                    builder.ins().uextend(lhs_ty, rhs)
                } else {
                    builder.ins().sextend(lhs_ty, rhs)
                };
                (lhs, rhs, lhs_ty)
            } else {
                let lhs = if is_unsigned {
                    builder.ins().uextend(rhs_ty, lhs)
                } else {
                    builder.ins().sextend(rhs_ty, lhs)
                };
                (lhs, rhs, rhs_ty)
            }
        } else if lhs_ty != rhs_ty && lhs_ty.is_float() && rhs_ty.is_float() {
            // Float widths differ — promote the narrower float.
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
                    builder.ins().trapz(rhs, TrapCode::INTEGER_DIVISION_BY_ZERO);
                    if is_unsigned {
                        builder.ins().udiv(lhs, rhs)
                    } else {
                        builder.ins().sdiv(lhs, rhs)
                    }
                }
            }
            BinOp::Rem => {
                if is_float {
                    // Float remainder via libcall to fmod/fmodf
                    let func_name = if ty == cl_types::F32 { "fmodf" } else { "fmod" };
                    let mut sig =
                        cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
                    sig.params.push(cranelift_codegen::ir::AbiParam::new(ty));
                    sig.params.push(cranelift_codegen::ir::AbiParam::new(ty));
                    sig.returns.push(cranelift_codegen::ir::AbiParam::new(ty));

                    let func_id = ctx
                        .module
                        .declare_function(func_name, Linkage::Import, &sig)
                        .map_err(|e| format!("Failed to declare {}: {}", func_name, e))?;
                    let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
                    let call = builder.ins().call(local_func, &[lhs, rhs]);
                    builder.inst_results(call)[0]
                } else {
                    builder.ins().trapz(rhs, TrapCode::INTEGER_DIVISION_BY_ZERO);
                    if is_unsigned {
                        builder.ins().urem(lhs, rhs)
                    } else {
                        builder.ins().srem(lhs, rhs)
                    }
                }
            }
            BinOp::BitAnd => builder.ins().band(lhs, rhs),
            BinOp::BitOr => builder.ins().bor(lhs, rhs),
            BinOp::BitXor => builder.ins().bxor(lhs, rhs),
            BinOp::Shl => builder.ins().ishl(lhs, rhs),
            BinOp::Shr => {
                if is_unsigned {
                    builder.ins().ushr(lhs, rhs)
                } else {
                    builder.ins().sshr(lhs, rhs)
                }
            }

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
                } else if is_unsigned {
                    builder.ins().icmp(IntCC::UnsignedLessThan, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs)
                }
            }
            BinOp::Le => {
                if is_float {
                    builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs)
                } else if is_unsigned {
                    builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs)
                }
            }
            BinOp::Gt => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs)
                } else if is_unsigned {
                    builder.ins().icmp(IntCC::UnsignedGreaterThan, lhs, rhs)
                } else {
                    builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs)
                }
            }
            BinOp::Ge => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs)
                } else if is_unsigned {
                    builder
                        .ins()
                        .icmp(IntCC::UnsignedGreaterThanOrEqual, lhs, rhs)
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
    fn operand_type_kind<'b>(operand: &'b Operand, type_ctx: &'b TypeCtx) -> &'b TypeKind {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                &type_ctx.local_types[place.local.0].kind
            }
            Operand::Constant(c) => &c.ty.kind,
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
    ) -> Result<Value, String> {
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
    ) -> Result<Value, String> {
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
