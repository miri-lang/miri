// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-counting code emission: drop/decref/clone thunk generation and
//! per-type field-walking drop logic. Runtime FFI call wrappers live in
//! `translator.rs`; this module dispatches into them.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::translator::{
    empty_module_ctx, is_field_managed, CallSite, ElementShape, FunctionTranslator, ModuleCtx,
    TypeCtx,
};
use crate::error::CodegenError;
use crate::runtime_fns::rt;
use crate::type_checker::context::{EnumDefinition, TypeDefinition};

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

impl<'a> FunctionTranslator<'a> {
    /// Address of the runtime decref helper for an element of `shape`, or
    /// `None` when the shape needs no decref (primitives, void, etc.).
    pub(crate) fn elem_decref_addr_for_shape(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        shape: ElementShape,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let addr = match shape {
            ElementShape::String => {
                Self::get_rt_string_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::List) => {
                Self::get_rt_list_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Array) => {
                Self::get_rt_array_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Set) => {
                Self::get_rt_set_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Map) => {
                Self::get_rt_map_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::UserClass(name) => {
                Self::get_custom_decref_thunk_addr(builder, ctx, name, ptr_type)?
            }
            ElementShape::Other => return Ok(None),
        };
        Ok(Some(addr))
    }

    /// Address of the runtime clone helper for an element of `shape`.
    ///
    /// Only user classes that implement `Cloneable` produce a clone helper;
    /// built-in collections and strings use the runtime's default IncRef path.
    pub(crate) fn elem_clone_addr_for_shape(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        shape: ElementShape,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let ElementShape::UserClass(name) = shape else {
            return Ok(None);
        };
        if !Self::class_implements_cloneable(name, type_definitions) {
            return Ok(None);
        }
        Ok(Some(Self::get_custom_clone_thunk_addr(
            builder, ctx, name, ptr_type,
        )?))
    }

    /// Sets `elem_drop_fn` on `list_ptr` based on the declared element type.
    ///
    /// Used when an empty `List<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    pub(crate) fn emit_list_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)? {
            Self::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_drop_fn` on `set_ptr` based on the declared element type.
    ///
    /// Used when an empty `Set<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    pub(crate) fn emit_set_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) = Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)? {
            Self::call_rt_set_set_elem_drop_fn(builder, ctx, set_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_clone_fn` on `list_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_list_drop_fn_for_elem_kind` but for the clone
    /// side. Called on the empty-constructor path where `translate_rvalue` has no
    /// operands to inspect.
    pub(crate) fn emit_list_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
            Self::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_clone_fn` on `set_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_set_drop_fn_for_elem_kind` but for the clone
    /// side.
    pub(crate) fn emit_set_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
            Self::call_rt_set_set_elem_clone_fn(builder, ctx, set_ptr, addr)?;
        }
        Ok(())
    }

    /// Emits a loop that DecRefs each managed value in a map's hash table.
    ///
    /// Iterates over all `capacity` slots, checks the state byte for SLOT_OCCUPIED (1),
    /// and DecRefs the value pointer in each occupied slot.
    ///
    /// MiriMap states array: 1 byte per slot (0=empty, 1=occupied, 2=tombstone).
    /// Values are stored as pointer-sized entries (managed types are always pointers).
    fn emit_map_managed_values_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        states: Value,
        values: Value,
        capacity: Value,
        val_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let check_block = builder.create_block();
        let decref_block = builder.create_block();
        let increment_block = builder.create_block();
        let after_loop = builder.create_block();

        // Enter loop with index 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        // Loop header: check i < capacity
        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            capacity,
        );
        builder
            .ins()
            .brif(in_range, check_block, &[], after_loop, &[]);

        // Check state[i] == SLOT_OCCUPIED (1)
        builder.switch_to_block(check_block);
        builder.seal_block(check_block);
        let state_addr = builder.ins().iadd(states, i);
        let state_i8 = builder
            .ins()
            .load(cl_types::I8, MemFlags::new(), state_addr, 0);
        let state = builder.ins().uextend(ptr_type, state_i8);
        let slot_occupied = builder.ins().iconst(ptr_type, 1);
        let is_occupied = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            state,
            slot_occupied,
        );
        builder
            .ins()
            .brif(is_occupied, decref_block, &[], increment_block, &[]);

        // DecRef the value in this slot
        builder.switch_to_block(decref_block);
        builder.seal_block(decref_block);
        let byte_offset = builder.ins().imul_imm(i, ptr_size);
        let val_addr = builder.ins().iadd(values, byte_offset);
        let val_ptr = builder.ins().load(ptr_type, MemFlags::new(), val_addr, 0);
        Self::emit_decref_value(builder, ctx, val_kind, val_ptr, type_ctx)?;
        builder.ins().jump(increment_block, &[]);

        // Increment i and loop back
        builder.switch_to_block(increment_block);
        builder.seal_block(increment_block);
        let next_i = builder.ins().iadd_imm(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        builder.seal_block(loop_header);
        builder.seal_block(after_loop);
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits a loop that DecRefs each managed element in a contiguous pointer array.
    ///
    /// Used when freeing a List or Array whose inner type is managed, so that the
    /// RC of each stored element is properly decremented before the container is freed.
    ///
    /// `data` — pointer to the element buffer (pointer-sized elements assumed).
    /// `len`  — number of elements.
    fn emit_managed_elements_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        data: Value,
        len: Value,
        inner_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let loop_body = builder.create_block();
        let after_loop = builder.create_block();

        // Jump into the loop with initial index = 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            len,
        );
        builder
            .ins()
            .brif(in_range, loop_body, &[], after_loop, &[]);

        // loop_body: load element, DecRef it, increment index
        builder.switch_to_block(loop_body);
        builder.seal_block(loop_body); // only predecessor: loop_header

        let byte_offset = builder.ins().imul_imm(i, ptr_size);
        let elem_addr = builder.ins().iadd(data, byte_offset);
        let elem_ptr = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
        Self::emit_decref_value(builder, ctx, inner_kind, elem_ptr, type_ctx)?;

        let next_i = builder.ins().iadd_imm(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        // Seal loop_header now that both predecessors are defined
        builder.seal_block(loop_header);

        builder.seal_block(after_loop); // only predecessor: loop_header
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits the type-appropriate cleanup when an object's RC reaches zero.
    ///
    /// All heap types share the same `[RC][payload]` layout, so the RC
    /// increment/decrement logic is uniform. The only type-specific part is
    /// *what* to free when RC hits zero:
    /// - Arrays/Lists call their runtime `_free` functions (which handle internal buffers).
    /// - Structs/enums with managed fields: DecRef each managed field, then free the block.
    /// - All other types just need the `[RC][payload]` block freed.
    ///
    /// `ptr` points to the payload (past the RC header).
    /// `header_ptr` points to the RC header (`ptr - ptr_size`).
    pub(crate) fn emit_type_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        // Resolve type aliases before dispatching so that e.g.
        // `type IntArray is [int; 2]` correctly frees via rt_array_free.
        let resolved = Self::resolve_alias(kind, type_ctx.type_definitions);
        let kind = resolved.unwrap_or(kind);

        if Self::is_map_type(kind) {
            Self::emit_drop_map(builder, ctx, kind, ptr, type_ctx)
        } else if Self::is_set_type(kind) {
            Self::call_rt_set_free(builder, ctx, ptr)
        } else if Self::is_list_type(kind) {
            Self::emit_drop_list_or_array(builder, ctx, kind, ptr, type_ctx, true)
        } else if Self::is_collection_type(kind) {
            Self::emit_drop_list_or_array(builder, ctx, kind, ptr, type_ctx, false)
        } else if let TypeKind::Tuple(element_exprs) = kind {
            Self::emit_drop_tuple(builder, ctx, kind, element_exprs, ptr, header_ptr, type_ctx)
        } else if let TypeKind::Option(inner) = kind {
            Self::emit_drop_option(builder, ctx, inner, ptr, header_ptr, type_ctx)
        } else if kind == &TypeKind::String {
            // miri_rt_string_free takes the payload pointer (not header) — it
            // calls free_with_rc internally.
            Self::call_rt_string_free(builder, ctx, ptr)
        } else if let TypeKind::Custom(name, _) = kind {
            Self::emit_drop_custom(builder, ctx, name, ptr, header_ptr, type_ctx)
        } else if matches!(kind, TypeKind::Function(_)) {
            Self::emit_drop_closure(builder, ctx, ptr, header_ptr, type_ctx)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// DecRef managed values stored in a `MiriMap`, then free the map struct.
    /// Map layout: `[states][keys][values][len][capacity]...` (ptr-sized fields).
    fn emit_drop_map(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if let Some(val_expr) = Self::map_val_expr(kind) {
            if let ExpressionKind::Type(val_ty, _) = &val_expr.node {
                if is_field_managed(&val_ty.kind) {
                    let ptr_type = type_ctx.ptr_type;
                    let ptr_size = ptr_type.bytes() as i32;
                    let states = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                    let values = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), ptr, 2 * ptr_size);
                    let capacity = builder
                        .ins()
                        .load(ptr_type, MemFlags::new(), ptr, 4 * ptr_size);
                    Self::emit_map_managed_values_drop_loop(
                        builder,
                        ctx,
                        states,
                        values,
                        capacity,
                        &val_ty.kind,
                        type_ctx,
                    )?;
                }
            }
        }
        Self::call_rt_map_free(builder, ctx, ptr)
    }

    /// DecRef managed elements before freeing a `MiriList` or `MiriArray`.
    ///
    /// Both layouts begin `[data: ptr][len/elem_count: ptr]...`. For Array, also
    /// zeros `elem_drop_fn` (slot 3 * ptr_size) so the runtime's free path does
    /// not run the per-element decref a second time.
    fn emit_drop_list_or_array(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
        is_list: bool,
    ) -> Result<(), CodegenError> {
        if let Some(inner_expr) = Self::collection_elem_expr(kind) {
            if let ExpressionKind::Type(inner_ty, _) = &inner_expr.node {
                if is_field_managed(&inner_ty.kind) {
                    let ptr_type = type_ctx.ptr_type;
                    let ptr_size = ptr_type.bytes() as i32;
                    let data = builder.ins().load(ptr_type, MemFlags::new(), ptr, 0);
                    let len = builder.ins().load(ptr_type, MemFlags::new(), ptr, ptr_size);
                    Self::emit_managed_elements_drop_loop(
                        builder,
                        ctx,
                        data,
                        len,
                        &inner_ty.kind,
                        type_ctx,
                    )?;
                    if !is_list {
                        // Array: zero elem_drop_fn (slot 3) so the runtime's free
                        // path does not call decref a second time on already-
                        // dropped elements.
                        let zero = builder.ins().iconst(ptr_type, 0);
                        builder
                            .ins()
                            .store(MemFlags::new(), zero, ptr, 3 * ptr_size);
                    }
                }
            }
        }
        if is_list {
            Self::call_rt_list_free(builder, ctx, ptr)
        } else {
            Self::call_rt_array_free(builder, ctx, ptr)
        }
    }

    /// DecRef each managed field of a tuple, then `free()` the RC block.
    /// Layout: `[elem_count: ptr][field0][field1]...` (payload_ptr = `ptr`).
    #[allow(clippy::too_many_arguments)]
    fn emit_drop_tuple(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        element_exprs: &[Expression],
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let tuple_type = kind.clone();
        let managed_fields: Vec<(i32, TypeKind)> = element_exprs
            .iter()
            .enumerate()
            .filter_map(|(i, expr)| {
                let ExpressionKind::Type(ty, _) = &expr.node else {
                    return None;
                };
                if !is_field_managed(&ty.kind) {
                    return None;
                }
                let (offset, _) =
                    layout::field_layout(&tuple_type, i, type_ctx.type_definitions, ptr_type);
                Some((offset, ty.kind.clone()))
            })
            .collect();
        for (offset, field_kind) in managed_fields {
            let field_ptr = builder.ins().load(ptr_type, MemFlags::new(), ptr, offset);
            Self::emit_decref_value(builder, ctx, &field_kind, field_ptr, type_ctx)?;
        }
        Self::call_libc_free(builder, ctx, header_ptr)
    }

    /// DecRef an Option's inner value (when managed), then free the RC block.
    fn emit_drop_option(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        inner: &Type,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if is_field_managed(&inner.kind) {
            let ptr_type = type_ctx.ptr_type;
            let cl_inner_ty =
                crate::codegen::cranelift::types::translate_type_kind(&inner.kind, ptr_type);
            let inner_ptr = builder.ins().load(cl_inner_ty, MemFlags::new(), ptr, 0);
            Self::emit_decref_value(builder, ctx, &inner.kind, inner_ptr, type_ctx)?;
        }
        Self::call_libc_free(builder, ctx, header_ptr)
    }

    /// Drop a custom struct/class/enum: dispatch through the type-specific
    /// `__drop_TypeName` thunk when it carries managed fields or a user-defined
    /// drop hook; otherwise free the RC block directly.
    fn emit_drop_custom(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let needs_thunk = Self::has_managed_fields(name, type_ctx.type_definitions)
            || Self::type_has_user_drop(name, type_ctx.type_definitions);
        if needs_thunk {
            Self::call_drop_thunk(builder, ctx, name, ptr, type_ctx.ptr_type)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// Drop a closure: invoke its `dtor_ptr` (when non-null) to DecRef captures,
    /// then decrement the closure-balance counter and free the closure struct.
    /// Layout: `payload[0]=fn_ptr, payload[1]=dtor_ptr, payload[2+i]=cap_i`.
    fn emit_drop_closure(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;
        let dtor_ptr = builder
            .ins()
            .load(ptr_type, MemFlags::new(), ptr, ptr_size as i32);
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            dtor_ptr,
            null,
        );
        let dtor_block = builder.create_block();
        let after_dtor = builder.create_block();
        builder
            .ins()
            .brif(is_null, after_dtor, &[], dtor_block, &[]);
        builder.switch_to_block(dtor_block);
        builder.seal_block(dtor_block);
        let mut dtor_sig = cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
        dtor_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let dtor_sig_ref = builder.import_signature(dtor_sig);
        builder.ins().call_indirect(dtor_sig_ref, dtor_ptr, &[ptr]);
        builder.ins().jump(after_dtor, &[]);
        builder.switch_to_block(after_dtor);
        builder.seal_block(after_dtor);
        Self::call_rt_closure_free_track(builder, ctx)?;
        Self::call_libc_free(builder, ctx, header_ptr)
    }

    /// Resolves a type alias to its underlying type kind.
    /// Returns `Some(resolved_kind)` if the type is an alias, `None` otherwise.
    fn resolve_alias<'b>(
        kind: &TypeKind,
        type_definitions: &'b HashMap<String, TypeDefinition>,
    ) -> Option<&'b TypeKind> {
        if let TypeKind::Custom(name, _) = kind {
            if let Some(TypeDefinition::Alias(alias_def)) = type_definitions.get(name) {
                // Recurse to handle chained aliases (A -> B -> [int])
                let inner = &alias_def.template.kind;
                Self::resolve_alias(inner, type_definitions).or(Some(inner))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Emits DecRef calls for all managed fields of a struct, class, or enum.
    ///
    /// For structs and classes, iterates all fields and emits a DecRef sequence
    /// for each managed (heap-allocated) field. For enums, reads the discriminant
    /// and conditionally DecRefs the active variant's managed fields.
    ///
    /// This is the body of the generated `__drop_TypeName` function and is also
    /// called directly from `generate_drop_function`.
    pub(crate) fn emit_struct_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let Some(def) = type_ctx.type_definitions.get(type_name) else {
            return Ok(());
        };
        match def {
            TypeDefinition::Struct(struct_def) => {
                let managed_fields: Vec<(usize, TypeKind)> = struct_def
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, ty, _))| is_field_managed(&ty.kind))
                    .map(|(idx, (_, ty, _))| (idx, ty.kind.clone()))
                    .collect();
                Self::emit_struct_like_field_decrefs(
                    builder,
                    ctx,
                    type_name,
                    &managed_fields,
                    payload_ptr,
                    type_ctx,
                )
            }
            TypeDefinition::Enum(enum_def) => {
                Self::emit_enum_drop(builder, ctx, enum_def, payload_ptr, type_ctx)
            }
            TypeDefinition::Class(class_def) => {
                use crate::type_checker::context::collect_class_fields_all;
                let all_fields = collect_class_fields_all(class_def, type_ctx.type_definitions);
                let managed_fields: Vec<(usize, TypeKind)> = all_fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, fi))| is_field_managed(&fi.ty.kind))
                    .map(|(idx, (_, fi))| (idx, fi.ty.kind.clone()))
                    .collect();
                Self::emit_struct_like_field_decrefs(
                    builder,
                    ctx,
                    type_name,
                    &managed_fields,
                    payload_ptr,
                    type_ctx,
                )
            }
            TypeDefinition::Trait(_) | TypeDefinition::Alias(_) | TypeDefinition::Generic(_) => {
                Ok(())
            }
        }
    }

    /// Emit `DecRef` for every managed field of a struct- or class-shaped
    /// type. `managed_fields` carries the (full-field-list) index and field
    /// type kind; offsets are resolved through `layout::field_layout` so
    /// inherited fields land at the right slot.
    fn emit_struct_like_field_decrefs(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        managed_fields: &[(usize, TypeKind)],
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let custom_kind = TypeKind::Custom(type_name.to_string(), None);
        for (field_idx, field_kind) in managed_fields {
            let (offset, _cl_ty) = layout::field_layout(
                &custom_kind,
                *field_idx,
                type_ctx.type_definitions,
                ptr_type,
            );
            let field_ptr = builder
                .ins()
                .load(ptr_type, MemFlags::new(), payload_ptr, offset);
            Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
        }
        Ok(())
    }

    /// Drop an enum payload. Reads the discriminant at offset 0 and, for each
    /// variant that carries managed fields, emits a guarded block that
    /// DecRefs the variant's fields when the discriminant matches.
    fn emit_enum_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        enum_def: &EnumDefinition,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let disc = builder
            .ins()
            .load(ptr_type, MemFlags::new(), payload_ptr, 0);

        for (variant_idx, managed_fields) in Self::enum_variants_with_managed_fields(enum_def) {
            Self::emit_enum_variant_drop_guard(
                builder,
                ctx,
                disc,
                variant_idx,
                &managed_fields,
                payload_ptr,
                type_ctx,
            )?;
        }
        Ok(())
    }

    /// Collect `(variant_idx, [(field_idx, field_kind), ...])` for every
    /// enum variant carrying at least one managed field. Variants without
    /// managed fields are filtered out so the caller only emits guarded
    /// blocks when there is decref work to do.
    fn enum_variants_with_managed_fields(
        enum_def: &EnumDefinition,
    ) -> Vec<(usize, Vec<(usize, TypeKind)>)> {
        enum_def
            .variants
            .iter()
            .enumerate()
            .filter_map(|(vi, (_name, fields))| {
                let managed: Vec<(usize, TypeKind)> = fields
                    .iter()
                    .enumerate()
                    .filter(|(_, ty)| is_field_managed(&ty.kind))
                    .map(|(fi, ty)| (fi, ty.kind.clone()))
                    .collect();
                if managed.is_empty() {
                    None
                } else {
                    Some((vi, managed))
                }
            })
            .collect()
    }

    /// Emit `if disc == variant_idx { decref each managed field }`. Caller
    /// continues in the merge block after this returns.
    #[allow(clippy::too_many_arguments)]
    fn emit_enum_variant_drop_guard(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        disc: Value,
        variant_idx: usize,
        managed_fields: &[(usize, TypeKind)],
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        let variant_val = builder.ins().iconst(ptr_type, variant_idx as i64);
        let is_this_variant = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            disc,
            variant_val,
        );

        let drop_block = builder.create_block();
        let merge_block = builder.create_block();
        builder
            .ins()
            .brif(is_this_variant, drop_block, &[], merge_block, &[]);

        builder.switch_to_block(drop_block);
        for (field_idx, field_kind) in managed_fields {
            let field_offset = ptr_size + (*field_idx as i32 * ptr_size);
            let field_ptr =
                builder
                    .ins()
                    .load(ptr_type, MemFlags::new(), payload_ptr, field_offset);
            Self::emit_decref_value(builder, ctx, field_kind, field_ptr, type_ctx)?;
        }
        builder.ins().jump(merge_block, &[]);
        builder.seal_block(drop_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(())
    }

    /// Decrements the RC of the existing element at `elem_addr` when the element
    /// type is a managed heap object (String, List, Array, Set, Map, or user-defined class).
    ///
    /// Called by `translate_collection_index_write` before the new value is stored
    /// so that overwriting an existing slot does not leak the old value.
    pub(crate) fn emit_managed_elem_decref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_addr: Value,
        elem_type_kind: &TypeKind,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_type_kind);
        let builtin_decref = match shape {
            ElementShape::String => Some(rt::STRING_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::List) => Some(rt::LIST_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Array) => Some(rt::ARRAY_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Set) => Some(rt::SET_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Map) => Some(rt::MAP_DECREF_ELEMENT),
            ElementShape::UserClass(_) | ElementShape::Other => None,
        };
        if let Some(name) = builtin_decref {
            let old_val = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
            Self::call_cached_func(
                builder,
                ctx.module,
                &mut ctx.cached_funcs,
                CallSite {
                    name,
                    param_types: &[ptr_type],
                    return_types: &[],
                    args: &[old_val],
                },
            )?;
            return Ok(());
        }
        let ElementShape::UserClass(class_name) = shape else {
            // Primitives need no decref.
            return Ok(());
        };
        let mut decref_name = String::with_capacity(9 + class_name.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(class_name);
        let old_val = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&decref_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[old_val]);
        Ok(())
    }

    /// Emits a call to the type-specific drop thunk `__drop_{type_name}(ptr)`.
    ///
    /// This is the sole call site for dropping a Custom type once RC reaches zero.
    /// The thunk function is declared as `Import` here and must be defined elsewhere
    /// (via `generate_drop_function`) before the final link.
    fn call_drop_thunk(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        type_name: &str,
        ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let mut thunk_name = String::with_capacity(7 + type_name.len());
        thunk_name.push_str("__drop_");
        thunk_name.push_str(type_name);
        let mut sig = Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = ctx
            .module
            .declare_function(&thunk_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(thunk_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
    }

    /// Emits an inline DecRef sequence for a managed value.
    ///
    /// Checks the RC header, decrements it, and if zero calls emit_type_drop
    /// recursively. This is the same logic as `StatementKind::DecRef` but for
    /// an arbitrary `Value` (not tied to a MIR local).
    pub(crate) fn emit_decref_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        // Guard: skip if pointer is null
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        builder.switch_to_block(rc_block);

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        let rc = builder.ins().load(
            ptr_type,
            cranelift_codegen::ir::MemFlags::new(),
            header_ptr,
            0,
        );

        // Skip immortal objects (RC < 0)
        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        builder.switch_to_block(dec_block);
        let new_rc = builder.ins().iadd_imm(rc, -1);
        builder.ins().store(
            cranelift_codegen::ir::MemFlags::new(),
            new_rc,
            header_ptr,
            0,
        );

        let zero = builder.ins().iconst(ptr_type, 0);
        let is_zero =
            builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, new_rc, zero);

        let free_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, free_block, &[], merge_block, &[]);

        builder.switch_to_block(free_block);
        Self::emit_type_drop(builder, ctx, kind, ptr, header_ptr, type_ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(rc_block);
        builder.seal_block(dec_block);
        builder.seal_block(free_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

        Ok(())
    }

    /// Generates the `__drop_{type_name}(ptr)` function in the given module.
    ///
    /// The generated function implements the three-step destructor pipeline:
    /// 1. User-defined drop hook — invoked when the type defines `fn drop(self)`.
    /// 2. Recursively DecRef all managed fields.
    /// 3. Free the RC allocation via `libc::free`.
    ///
    /// This function is called once per managed concrete type during codegen,
    /// before any user functions are compiled, so the thunk symbols are available
    /// when user code later references them via Import declarations.
    pub(crate) fn generate_drop_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut func_name = String::with_capacity(7 + type_name.len());
        func_name.push_str("__drop_");
        func_name.push_str(type_name);
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&func_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_drop_body(
            module,
            ctx,
            &mut builder_ctx,
            type_name,
            type_definitions,
            ptr_type,
            call_conv,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(func_name, e.to_string()))?;
        ctx.clear();

        // Generate __decref_TypeName: the RC-decrement wrapper used as
        // elem_drop_fn for collections holding custom-type elements.
        Self::generate_decref_function(module, ctx, isa, type_name)
    }

    /// Emit the body of `__drop_TypeName(ptr)`:
    ///   1. invoke user-defined `fn drop(self)` when the type defines one,
    ///   2. DecRef every managed field via `emit_struct_drop`,
    ///   3. free the RC allocation via libc `free`.
    #[allow(clippy::too_many_arguments)]
    fn emit_drop_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            type_definitions,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
        };

        if Self::type_has_user_drop(type_name, type_definitions) {
            Self::call_user_drop_hook(
                &mut builder,
                &mut module_ctx,
                type_name,
                ptr,
                ptr_type,
                call_conv,
            )?;
        }

        Self::emit_struct_drop(&mut builder, &mut module_ctx, type_name, ptr, &type_ctx)?;

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        Self::call_libc_free(&mut builder, &mut module_ctx, header_ptr)?;

        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Emit a call to the user-defined `{TypeName}_drop(self, allocator)` hook
    /// declared by `fn drop(self)`. ABI mirrors `lower_class_method`:
    /// `(self: ptr, allocator: ptr) -> void`, with a null allocator placeholder.
    fn call_user_drop_hook(
        builder: &mut FunctionBuilder,
        module_ctx: &mut ModuleCtx,
        type_name: &str,
        self_ptr: Value,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let mut user_drop_name = String::with_capacity(type_name.len() + 5);
        user_drop_name.push_str(type_name);
        user_drop_name.push_str("_drop");
        let mut user_sig = Signature::new(call_conv);
        user_sig.params.push(AbiParam::new(ptr_type));
        user_sig.params.push(AbiParam::new(ptr_type));
        let user_drop_id = module_ctx
            .module
            .declare_function(&user_drop_name, Linkage::Import, &user_sig)
            .map_err(|e| CodegenError::declare_function(user_drop_name, e.to_string()))?;
        let local_user_drop = module_ctx
            .module
            .declare_func_in_func(user_drop_id, builder.func);
        let zero = builder.ins().iconst(ptr_type, 0);
        builder.ins().call(local_user_drop, &[self_ptr, zero]);
        Ok(())
    }

    /// Generates `__decref_{type_name}(ptr)` in the given module.
    ///
    /// Emits the RC-decrement pattern:
    ///   1. Guard: skip if ptr is null.
    ///   2. Guard: skip if RC < 0 (immortal).
    ///   3. Decrement RC.
    ///   4. If RC reaches zero, call `__drop_{type_name}(ptr)`.
    ///
    /// Used as `elem_drop_fn` for List/Set/Map holding custom-type elements so
    /// that mutation operations (clear, remove_at, remove) properly DecRef
    /// each removed instance.
    fn generate_decref_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut decref_name = String::with_capacity(9 + type_name.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(type_name);
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&decref_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_decref_body(module, ctx, &mut builder_ctx, type_name, ptr_type, &sig)?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(decref_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `__decref_TypeName`: null guard → immortal guard →
    /// decrement RC → when RC hits zero call `__drop_TypeName(ptr)` → return.
    fn emit_decref_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        ptr_type: cl_types::Type,
        sig: &Signature,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        // Null guard.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        // Load RC + check immortal flag (high bit set).
        builder.switch_to_block(rc_block);
        builder.seal_block(rc_block);
        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        let rc = builder.ins().load(ptr_type, MemFlags::new(), header_ptr, 0);
        let is_immortal = builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        // Decrement RC; branch to `__drop` thunk when it reaches zero.
        builder.switch_to_block(dec_block);
        builder.seal_block(dec_block);
        let new_rc = builder.ins().iadd_imm(rc, -1);
        builder.ins().store(MemFlags::new(), new_rc, header_ptr, 0);
        let zero = builder.ins().iconst(ptr_type, 0);
        let is_zero =
            builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, new_rc, zero);
        let free_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, free_block, &[], merge_block, &[]);

        // `__drop_TypeName(ptr)` call site.
        builder.switch_to_block(free_block);
        builder.seal_block(free_block);
        let mut drop_name = String::with_capacity(7 + type_name.len());
        drop_name.push_str("__drop_");
        drop_name.push_str(type_name);
        let drop_func_id = module
            .declare_function(&drop_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(drop_name.clone(), e.to_string()))?;
        let local_drop = module.declare_func_in_func(drop_func_id, builder.func);
        builder.ins().call(local_drop, &[ptr]);
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        builder.ins().return_(&[]);
        builder.finalize();
        Ok(())
    }

    /// Generates `__clone_{type_name}(ptr) -> ptr` for each concrete class that
    /// implements `Cloneable`.
    ///
    /// This function is used as `elem_clone_fn` in Array/List/Set so that
    /// `miri_rt_XXX_clone` produces independent element copies instead of just
    /// IncRef-ing shared pointers.  It delegates to the user's compiled `clone()`
    /// method (`{TypeName}_clone`), which already encodes any deep-copy logic.
    ///
    /// Only generated for concrete (non-generic, non-abstract) classes whose
    /// `traits` list includes `"Cloneable"` (checked with inheritance walk).
    pub(crate) fn generate_clone_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        // Only generate for concrete classes that implement Cloneable somewhere in their
        // hierarchy. Abstract classes may have an abstract clone() with no compiled body;
        // generating a thunk for them would reference an undefined symbol at link time.
        match type_definitions.get(type_name) {
            Some(TypeDefinition::Class(cd)) if cd.is_abstract => return Ok(()),
            None
            | Some(TypeDefinition::Class(_))
            | Some(TypeDefinition::Struct(_))
            | Some(TypeDefinition::Enum(_))
            | Some(TypeDefinition::Generic(_))
            | Some(TypeDefinition::Alias(_))
            | Some(TypeDefinition::Trait(_)) => {}
        }
        if !Self::class_implements_cloneable(type_name, type_definitions) {
            return Ok(());
        }

        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mut clone_name = String::with_capacity(9 + type_name.len());
        clone_name.push_str("__clone_");
        clone_name.push_str(type_name);

        // Signature: (ptr: *TypeName) -> *TypeName
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns.push(AbiParam::new(ptr_type));

        let func_id = module
            .declare_function(&clone_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(clone_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_clone_body(
            module,
            ctx,
            &mut builder_ctx,
            type_name,
            type_definitions,
            ptr_type,
            call_conv,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(clone_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `__clone_TypeName(ptr)`: null guard → call the
    /// user-defined `clone()` resolved through the inheritance chain →
    /// return the result.
    #[allow(clippy::too_many_arguments)]
    fn emit_clone_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        // Null guard: return null if ptr is null.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let null_ret_block = builder.create_block();
        let call_block = builder.create_block();
        builder
            .ins()
            .brif(is_null, null_ret_block, &[], call_block, &[]);

        builder.switch_to_block(null_ret_block);
        builder.seal_block(null_ret_block);
        builder.ins().return_(&[null]);

        builder.switch_to_block(call_block);
        builder.seal_block(call_block);

        // Resolve clone() through inheritance (applies concrete-caller / abstract-definer rule).
        let clone_method_name = Self::resolve_clone_method_name(type_name, type_definitions);
        let mut user_clone_sig = Signature::new(call_conv);
        user_clone_sig.params.push(AbiParam::new(ptr_type)); // self
        user_clone_sig.params.push(AbiParam::new(ptr_type)); // allocator
        user_clone_sig.returns.push(AbiParam::new(ptr_type));

        let user_clone_id = module
            .declare_function(&clone_method_name, Linkage::Import, &user_clone_sig)
            .map_err(|e| CodegenError::declare_function(clone_method_name, e.to_string()))?;
        let local_fn = module.declare_func_in_func(user_clone_id, builder.func);
        let zero = builder.ins().iconst(ptr_type, 0);
        let inst = builder.ins().call(local_fn, &[ptr, zero]);
        let result = builder.inst_results(inst)[0];
        builder.ins().return_(&[result]);

        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Resolves the mangled name of the `clone()` method for `type_name`.
    ///
    /// Walks the inheritance chain to find where `clone()` is defined.  The
    /// concrete-caller / abstract-definer rule is applied: if the defining class
    /// is abstract, the caller's name is used instead (matching how
    /// `resolve_inherited_method` in `mir::lowering::dispatch` mangles the call).
    fn resolve_clone_method_name(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> String {
        crate::mir::lowering::dispatch::resolve_inherited_method(
            type_definitions,
            type_name,
            "clone",
        )
        .map(|(defining, _)| format!("{defining}_clone"))
        .unwrap_or_else(|| format!("{type_name}_clone"))
    }

    /// Returns true if `type_name` (or any ancestor class) lists `"Cloneable"` in its traits.
    pub(crate) fn class_implements_cloneable(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        let mut current = type_name.to_string();
        loop {
            match type_definitions.get(&current) {
                Some(TypeDefinition::Class(cd)) => {
                    if cd
                        .traits
                        .iter()
                        .any(|t| t == crate::ast::types::CLONEABLE_TRAIT_NAME)
                    {
                        return true;
                    }
                    match &cd.base_class {
                        Some(base) => current = base.clone(),
                        None => return false,
                    }
                }
                Some(
                    TypeDefinition::Struct(_)
                    | TypeDefinition::Enum(_)
                    | TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_)
                    | TypeDefinition::Trait(_),
                )
                | None => return false,
            }
        }
    }
}
