// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-counting code emission: drop/decref/clone thunk generation and
//! per-type field-walking drop logic. Runtime FFI call wrappers live in
//! `translator.rs`; this module dispatches into them.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::factory::type_expr_non_null;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::element_method_thunks::ElementMethod;
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::translator::{
    empty_module_ctx, CallSite, ElementShape, FunctionTranslator, ModuleCtx, TypeCtx,
};
use crate::error::CodegenError;
use crate::mir::rc::{is_field_managed, is_word_slot_managed};
use crate::runtime_fns::rt;
use crate::type_checker::context::{EnumDefinition, TypeDefinition};

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlags, Signature, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// A runtime call recording one pointer-sized setting on a container.
pub(crate) type ContainerSetter =
    fn(&mut FunctionBuilder, &mut ModuleCtx, Value, Value) -> Result<(), CodegenError>;

/// The runtime setters through which a set's elements, or a map's keys, learn
/// how two of them are matched. See [`FunctionTranslator::emit_element_identity`].
#[derive(Clone, Copy)]
pub(crate) struct ElementIdentitySetters {
    /// Selects the built-in rule: raw bytes or string content.
    pub(crate) set_kind: ContainerSetter,
    /// Routes matching through a generated `equals` thunk.
    pub(crate) set_equals_fn: ContainerSetter,
}

/// The rule by which a container recognises two of its elements, or a map two
/// of its keys, as the same one.
#[derive(Clone, Copy)]
pub(crate) enum ElementRule {
    /// The bytes of the value are the value.
    Bytes,
    /// The value points at a string, matched by its content.
    StringContent,
    /// The value's type answers `equals`, reached through the thunk at this
    /// address.
    OwnEquals(Value),
}

/// The runtime setters through which a list's or array's elements learn how
/// two of them are ordered. See [`FunctionTranslator::emit_element_order`].
#[derive(Clone, Copy)]
pub(crate) struct ElementOrderSetters {
    /// Selects how an element's bytes read as its value: signed, unsigned or float.
    pub(crate) set_kind: ContainerSetter,
    /// Routes ordering through a generated `compare` thunk.
    pub(crate) set_compare_fn: ContainerSetter,
}

/// The enum value one variant's drop guard releases fields out of.
#[derive(Clone, Copy)]
struct EnumDropSite {
    /// The discriminant, read once for every variant's guard.
    disc: Value,
    /// Address of the discriminant slot.
    payload_ptr: Value,
    /// Width of each slot, from [`layout::enum_payload_slot_size`].
    slot_size: i32,
}

/// Mangle a generic class name with a concrete instantiation's type arguments,
/// producing the per-instantiation drop-thunk suffix (`Box` + `[String]` →
/// `Box__String`). Shares `mangle_generic_name`'s scheme so the drop call site
/// and the thunk-generation site agree byte-for-byte; the parameter names are
/// irrelevant to the mangling, so an empty placeholder name is used.
pub fn mangle_class_instantiation(class_name: &str, type_args: &[Type]) -> String {
    crate::mir::lowering::dispatch::mangle_instantiation_name(class_name, type_args)
}

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

    /// Address of the decref helper to register as a collection's `elem_drop_fn`
    /// for element type `elem_kind`, or `None` when the element needs no decref.
    ///
    /// A recorded generic-class instantiation (`Box<String>`) routes to its
    /// per-instantiation `__decref_Box__String` wrapper so the concrete managed
    /// field is released when the runtime drops an element (`clear`, `remove_at`,
    /// `pop`). A structural element — a tuple, an option, a function value —
    /// routes to the thunk generated for its structure, since it has no
    /// declaration to name. All other shapes — including non-generic classes
    /// and unrecorded instantiations — fall back to the shared per-shape helper.
    pub(crate) fn elem_decref_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        if let Some(symbol) =
            crate::codegen::cranelift::structural_elements::structural_thunk_symbol(elem_kind)
        {
            let addr = Self::get_custom_decref_thunk_addr(builder, ctx, &symbol, ptr_type)?;
            return Ok(Some(addr));
        }
        let shape = Self::classify_element_shape(elem_kind);
        if let ElementShape::UserClass(class_name) = shape {
            // Every caller is expected to have screened the element already. If
            // one has not, refuse here rather than name a release helper for a
            // parameter that has no concrete type yet: that symbol is defined
            // nowhere, so emitting it turns a compile into a link failure
            // reporting a mangled name instead of the program.
            if Self::is_unresolved_generic_elem(elem_kind, type_ctx.type_definitions) {
                return Err(CodegenError::Internal(format!(
                    "refusing to register a release helper named for the unresolved generic \
                     parameter '{class_name}': the registration site must skip an element whose \
                     type is not yet concrete, as the others do"
                )));
            }
            let symbol = Self::generic_drop_thunk_name_part(
                class_name,
                Self::custom_type_args(elem_kind),
                type_ctx,
            );
            let addr = Self::get_custom_decref_thunk_addr(builder, ctx, &symbol, ptr_type)?;
            return Ok(Some(addr));
        }
        Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)
    }

    /// Address of the decref helper to register as a map's `key_drop_fn` for key
    /// type `key_kind`, or `None` when the key is a value type the map stores by
    /// its bytes and never releases.
    ///
    /// A key reaches the same helper a collection element of that type would;
    /// the separate entry point exists to skip a generic parameter that has no
    /// concrete symbol yet, the way the value side already does.
    pub(crate) fn key_decref_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        if Self::is_unresolved_generic_elem(key_kind, type_ctx.type_definitions) {
            return Ok(None);
        }
        Self::elem_decref_addr_for_kind(builder, ctx, key_kind, ptr_type, type_ctx)
    }

    /// The type-argument expressions of a `TypeKind::Custom`, or `None` for any
    /// other kind. Used to resolve a collection element's generic instantiation.
    fn custom_type_args(kind: &TypeKind) -> Option<&[Expression]> {
        if let TypeKind::Custom(_, args) = kind {
            args.as_deref()
        } else {
            None
        }
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
    ///
    /// `elem_kind` may be a generic placeholder (e.g. `T` inside a generic class
    /// method) — in that case there is no concrete `__decref_T` to register, so
    /// the runtime keeps its default no-op elem_drop_fn.
    pub(crate) fn emit_list_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if Self::is_unresolved_generic_elem(elem_kind, type_ctx.type_definitions) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_decref_addr_for_kind(builder, ctx, elem_kind, type_ctx.ptr_type, type_ctx)?
        {
            Self::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// True when `elem_kind` is a generic placeholder that has no concrete
    /// `__decref_TypeName` symbol — either `TypeKind::Generic`, or a
    /// `TypeKind::Custom(name, _)` whose `name` is unknown to the type-definition
    /// table or known only as `TypeDefinition::Generic`. This guards the
    /// elem-drop-fn override sites where emitting a reference to an undefined
    /// symbol would later fail at link time.
    pub fn is_unresolved_generic_elem(
        elem_kind: &TypeKind,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        match elem_kind {
            TypeKind::Generic(_, _, _) => true,
            TypeKind::Custom(name, _) => {
                if BuiltinCollectionKind::from_name(name).is_some() {
                    return false;
                }
                match type_definitions.get(name) {
                    None | Some(TypeDefinition::Generic(_)) => true,
                    Some(_) => false,
                }
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
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Meta(_)
            | TypeKind::Option(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => false,
        }
    }

    /// Address of the comparator to register as a container's `elem_compare_fn`
    /// for element type `elem_kind`, or `None` when the element's bytes are its
    /// value and the runtime orders them itself.
    ///
    /// The element type is named the way the collection's decref path names it:
    /// a string through its class, a custom type through its own name, and a
    /// recorded instantiation of a generic class through that instantiation, so
    /// the comparison runs the body compiled for the element's concrete type.
    fn elem_compare_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        let name = match Self::classify_element_shape(elem_kind) {
            ElementShape::String => crate::ast::types::STRING_TYPE_NAME,
            ElementShape::UserClass(name) => name,
            ElementShape::Builtin(_) | ElementShape::Other => return Ok(None),
        };
        if !ElementMethod::Compare.is_answered_by(name, type_ctx.type_definitions) {
            return Ok(None);
        }
        let symbol =
            Self::element_method_thunk_name_part(name, Self::custom_type_args(elem_kind), type_ctx);
        Ok(Some(Self::get_custom_compare_thunk_addr(
            builder,
            ctx,
            &symbol,
            type_ctx.ptr_type,
        )?))
    }

    /// Address of the equality to register on a set or map for elements of
    /// `elem_kind`, or `None` when the element type defines no `equals` of its
    /// own and elements are matched by the rule their bytes imply.
    ///
    /// The element type is named the way the comparator path names it, so a
    /// recorded instantiation of a generic class reaches the `equals` compiled
    /// for its concrete type.
    fn elem_equals_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        let ElementShape::UserClass(name) = Self::classify_element_shape(elem_kind) else {
            return Ok(None);
        };
        if !ElementMethod::Equals.is_answered_by(name, type_ctx.type_definitions) {
            return Ok(None);
        }
        let symbol =
            Self::element_method_thunk_name_part(name, Self::custom_type_args(elem_kind), type_ctx);
        Ok(Some(Self::get_custom_equals_thunk_addr(
            builder,
            ctx,
            &symbol,
            type_ctx.ptr_type,
        )?))
    }

    /// Registers how a set recognises two elements, or a map two keys, of
    /// `elem_kind` as the same one: a string by its content, a class through its
    /// own `equals`, anything else by its bytes (the runtime default).
    ///
    /// An optional element is none of those itself — its bytes are the address
    /// of its `Some` box — so it registers the rule of the value it wraps,
    /// together with how many boxes the runtime must open to reach that value
    /// and how wide the value is. A rule is registered only where it is the one
    /// `==` applies; an element whose equality is a walk over fields keeps the
    /// byte rule rather than being matched half-right.
    ///
    /// This is the one place both containers take the rule from. It has to run
    /// before the first element is stored, since the runtime places each element
    /// by the hash of the rule in force when it arrives.
    pub(crate) fn emit_element_identity(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        container_ptr: Value,
        type_ctx: &TypeCtx,
        setters: ElementIdentitySetters,
    ) -> Result<(), CodegenError> {
        if Self::is_unresolved_generic_elem(elem_kind, type_ctx.type_definitions) {
            return Ok(());
        }
        let (depth, value_kind) = Self::peel_optionals(elem_kind);
        let Some(rule) = Self::value_identity_rule(builder, ctx, value_kind, type_ctx)? else {
            return Ok(());
        };
        if let ElementRule::OwnEquals(addr) = rule {
            (setters.set_equals_fn)(builder, ctx, container_ptr, addr)?;
        }
        let Some(word) = Self::element_identity_kind(rule, depth, value_kind, type_ctx.ptr_type)
        else {
            return Ok(());
        };
        let kind = builder.ins().iconst(type_ctx.ptr_type, word);
        (setters.set_kind)(builder, ctx, container_ptr, kind)
    }

    /// The rule matching two values of `value_kind` the way `==` compares them,
    /// or `None` when no rule the runtime can apply does.
    ///
    /// A type whose equality is a structural walk over its fields — a struct, an
    /// enum, a collection — has no such rule: the runtime sees only bytes, and
    /// its bytes are an address.
    ///
    /// TODO: so a set of structs holds two values its own `==` calls one, and
    /// finds neither by an equal value built separately, with nothing reported.
    /// Reaching the walk needs it to have a linkable symbol, which no
    /// synthesized equality has today.
    fn value_identity_rule(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        value_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<Option<ElementRule>, CodegenError> {
        if let Some(addr) = Self::elem_equals_addr_for_kind(builder, ctx, value_kind, type_ctx)? {
            return Ok(Some(ElementRule::OwnEquals(addr)));
        }
        if matches!(
            Self::classify_element_shape(value_kind),
            ElementShape::String
        ) {
            return Ok(Some(ElementRule::StringContent));
        }
        Ok(Self::is_matched_by_bytes(value_kind).then_some(ElementRule::Bytes))
    }

    /// The element kind word registering `rule` for a value wrapped in `depth`
    /// optionals, or `None` when there is nothing to register: an unwrapped
    /// value matched by its bytes is what every container already starts at,
    /// and an equality callback settles matching on its own.
    fn element_identity_kind(
        rule: ElementRule,
        depth: usize,
        value_kind: &TypeKind,
        ptr_type: cl_types::Type,
    ) -> Option<i64> {
        let base = match rule {
            ElementRule::StringContent => Self::STRING_CONTENT_ELEMENT_KIND,
            ElementRule::Bytes | ElementRule::OwnEquals(_) => Self::BYTES_ELEMENT_KIND,
        };
        if depth == 0 {
            return (base != Self::BYTES_ELEMENT_KIND).then_some(base);
        }
        let value_size =
            crate::codegen::cranelift::types::translate_type_kind(value_kind, ptr_type).bytes();
        Self::optional_element_kind(base, depth, value_size)
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
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if Self::is_unresolved_generic_elem(elem_kind, type_ctx.type_definitions) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_decref_addr_for_kind(builder, ctx, elem_kind, type_ctx.ptr_type, type_ctx)?
        {
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
        if Self::is_unresolved_generic_elem(elem_kind, type_definitions) {
            return Ok(());
        }
        let shape = Self::classify_element_shape(elem_kind);
        if let Some(addr) =
            Self::elem_clone_addr_for_shape(builder, ctx, shape, type_definitions, ptr_type)?
        {
            Self::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Registers how a list or array orders its elements of `elem_kind`: a
    /// class or string through its own `compare`, an unsigned integer or a
    /// float by the value its bytes hold, anything else by the runtime's signed
    /// reading of those bytes (the default, which registers nothing).
    pub(crate) fn emit_element_order(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        container_ptr: Value,
        type_ctx: &TypeCtx,
        setters: ElementOrderSetters,
    ) -> Result<(), CodegenError> {
        if Self::is_unresolved_generic_elem(elem_kind, type_ctx.type_definitions) {
            return Ok(());
        }
        if let Some(kind) = Self::element_order_kind(elem_kind) {
            let kind = builder.ins().iconst(type_ctx.ptr_type, kind);
            (setters.set_kind)(builder, ctx, container_ptr, kind)?;
        }
        if let Some(addr) = Self::elem_compare_addr_for_kind(builder, ctx, elem_kind, type_ctx)? {
            (setters.set_compare_fn)(builder, ctx, container_ptr, addr)?;
        }
        Ok(())
    }

    /// The order setters of a list, for [`Self::emit_element_order`].
    pub(crate) const LIST_ORDER_SETTERS: ElementOrderSetters = ElementOrderSetters {
        set_kind: Self::call_rt_list_set_elem_order_kind,
        set_compare_fn: Self::call_rt_list_set_elem_compare_fn,
    };

    /// The order setters of an array, for [`Self::emit_element_order`].
    pub(crate) const ARRAY_ORDER_SETTERS: ElementOrderSetters = ElementOrderSetters {
        set_kind: Self::call_rt_array_set_elem_order_kind,
        set_compare_fn: Self::call_rt_array_set_elem_compare_fn,
    };

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
        if Self::is_unresolved_generic_elem(elem_kind, type_definitions) {
            return Ok(());
        }
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
        } else if let TypeKind::Custom(name, args) = kind {
            Self::emit_drop_custom(
                builder,
                ctx,
                name,
                args.as_deref(),
                ptr,
                header_ptr,
                type_ctx,
            )
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
                if !is_word_slot_managed(&ty.kind) {
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
        if is_word_slot_managed(&inner.kind) {
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
    /// drop hook; otherwise, for enums, emit field drops inline and free the RC
    /// block, or just free the block for non-enums.
    fn emit_drop_custom(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        type_args: Option<&[Expression]>,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        // Extract Type arguments from Expression arguments for generic enums.
        // This enables resolution of generic variant fields to their concrete
        // kinds so managed fields are correctly identified.
        let concrete_args = Self::extract_type_args_from_exprs(type_args);

        // A generic class instantiated at a recorded set of type arguments
        // dispatches to its per-instantiation drop thunk (`__drop_Box__String`)
        // so a managed field is DecRef'd and a scalar field skipped, each per
        // instantiation. Non-generic types and unrecorded instantiations use
        // the bare `__drop_Name` thunk.
        let thunk_target = Self::generic_drop_thunk_name_part(name, type_args, type_ctx);
        // A recorded generic instantiation always routes through its thunk: the
        // class's declared field kinds see only the bare generic `T` (never
        // managed), so `has_managed_fields` cannot detect a managed field that
        // exists only after substitution (`Box<String>`). The per-instantiation
        // thunk resolves the concrete field and frees the block either way.
        let is_per_instantiation = thunk_target != name;
        let needs_thunk = is_per_instantiation
            || Self::has_managed_fields(name, type_ctx.type_definitions)
            || Self::type_has_user_drop(name, type_ctx.type_definitions);
        if needs_thunk {
            Self::call_drop_thunk(builder, ctx, &thunk_target, ptr, type_ctx.ptr_type)
        } else if concrete_args.is_some() {
            // For generic enums (concrete_args available) without a thunk, emit field
            // drops inline using resolved type arguments so that generic variant
            // fields become their concrete kinds and are correctly identified as managed.
            if let Some(TypeDefinition::Enum(_)) = type_ctx.type_definitions.get(name) {
                Self::emit_struct_drop(
                    builder,
                    ctx,
                    name,
                    concrete_args.as_deref(),
                    ptr,
                    type_ctx,
                )?;
            }
            Self::call_libc_free(builder, ctx, header_ptr)
        } else {
            Self::call_libc_free(builder, ctx, header_ptr)
        }
    }

    /// The suffix of the element-method thunk to register for elements of
    /// `class_name`: the recorded instantiation's, as the drop path names it,
    /// when codegen emitted a thunk for that instantiation, else the shared one.
    ///
    /// Thunks exist only for instantiations the pipeline monomorphizes, since
    /// each calls a method body lowered for its arguments. A body still written
    /// against a parameter — the shared copy of a generic function holding a
    /// `Set<Box<T>>` — records `Box<T>` as well, and naming its instantiation
    /// here would reference a symbol nothing defines.
    ///
    /// For the same reason this is only ever asked about a class: a
    /// per-instantiation method thunk is emitted for no other kind. Every
    /// caller establishes that first, by asking whether the type answers the
    /// method at all, so nothing re-checks it here. A kind that starts
    /// answering one must gain its per-instantiation thunks in the same pass,
    /// or the name built below will reference a symbol nothing defines.
    fn element_method_thunk_name_part(
        class_name: &str,
        type_args: Option<&[Expression]>,
        type_ctx: &TypeCtx,
    ) -> String {
        // Every argument has to be a written type: a body is monomorphized for
        // types, so an instantiation carrying a value — the size of a value
        // generic — gets no per-instantiation method body and must be named at
        // the shared one.
        let written = match type_args {
            None => Vec::new(),
            Some(args) => match Self::extract_type_args_from_exprs(Some(args)) {
                Some(written) => written,
                None => return class_name.to_string(),
            },
        };
        let monomorphized = written.iter().all(|arg| {
            crate::mir::lowering::is_monomorphizable_type_argument(
                &arg.kind,
                type_ctx.type_definitions,
            )
        });
        if !monomorphized {
            return class_name.to_string();
        }
        Self::generic_drop_thunk_name_part(class_name, type_args, type_ctx)
    }

    /// The type an instantiation argument stands for, as the registry recorded
    /// it: a written type, or the marker a value argument denotes.
    ///
    /// A value-generic class is recorded at arguments that include the size,
    /// wrapped in a marker type. Skipping the value here would mangle a
    /// different name than the thunk generated for that instantiation, and the
    /// call would fall back to the shared thunk — which skips a field still
    /// written at a parameter, leaving a managed one unreleased.
    fn instantiation_argument(arg: &Expression) -> Option<Type> {
        if let ExpressionKind::Type(ty, _) = &arg.node {
            return Some((**ty).clone());
        }
        crate::type_checker::generics::value_generic_slot(arg)
    }

    /// Extract Type arguments from Expression type arguments.
    /// Returns `None` if no args or extraction fails; `Some(Vec)` otherwise.
    pub(crate) fn extract_type_args_from_exprs(
        type_args: Option<&[Expression]>,
    ) -> Option<Vec<Type>> {
        let args = type_args?;
        let mut concrete: Vec<Type> = Vec::with_capacity(args.len());
        for arg in args {
            let ExpressionKind::Type(ty, _) = &arg.node else {
                return None;
            };
            concrete.push((**ty).clone());
        }
        Some(concrete)
    }

    /// The `__drop_` suffix to call for a Custom type: the mangled
    /// `Box__String` for a generic type whose instantiation is recorded, else
    /// the bare `Box`.
    ///
    /// A generic struct and a generic enum mangle exactly as a generic class
    /// does. All three declare fields whose types are written in their own
    /// parameters, so the field a given instantiation stores is known only once
    /// the arguments are substituted, and each gets its own thunk.
    ///
    /// Gated on the instantiation registry so the emitted call always targets a
    /// thunk `generate_type_drop_functions` actually defined — both sides mangle
    /// through the same `mangle_generic_name`, so a registry hit guarantees the
    /// symbol exists.
    fn generic_drop_thunk_name_part(
        class_name: &str,
        type_args: Option<&[Expression]>,
        type_ctx: &TypeCtx,
    ) -> String {
        let Some(definition) = type_ctx.type_definitions.get(class_name) else {
            return class_name.to_string();
        };
        if definition.generics().is_none() {
            return class_name.to_string();
        }
        let Some(args) = type_args else {
            return class_name.to_string();
        };
        let mut concrete: Vec<Type> = Vec::with_capacity(args.len());
        for arg in args {
            let Some(ty) = Self::instantiation_argument(arg) else {
                return class_name.to_string();
            };
            concrete.push(ty);
        }
        let want = mangle_class_instantiation(class_name, &concrete);
        let recorded = type_ctx
            .generic_class_instantiations
            .get(class_name)
            .is_some_and(|tuples| {
                tuples
                    .iter()
                    .any(|tuple| mangle_class_instantiation(class_name, tuple) == want)
            });
        if recorded {
            want
        } else {
            class_name.to_string()
        }
    }

    /// Drop a closure: invoke its `dtor_ptr` (when non-null) to DecRef captures,
    /// then decrement the closure-balance counter and free the closure struct.
    /// Layout: `payload[0]=fn_ptr, payload[1]=dtor_ptr`, then the captures
    /// (see `CaptureLayout`).
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
    pub fn resolve_alias<'b>(
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
        inst_args: Option<&[Type]>,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let Some(def) = type_ctx.type_definitions.get(type_name) else {
            return Ok(());
        };
        match def {
            TypeDefinition::Struct(struct_def) => {
                let managed_fields =
                    Self::managed_struct_fields(struct_def, inst_args, type_ctx.type_definitions);
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
                Self::emit_enum_drop(builder, ctx, enum_def, inst_args, payload_ptr, type_ctx)
            }
            TypeDefinition::Class(class_def) => {
                // A field of a generic class is written in the type parameters
                // of the class that declares it (`value T`, `items List<T>`),
                // which name nothing concrete on their own. The
                // per-instantiation drop thunk (`__drop_Box__String`) supplies
                // `inst_args`, and the `extends` chain carries them on to every
                // ancestor, so each field resolves to the kind this instance
                // actually stores: a managed one joins the DecRef set at that
                // kind, a scalar one is a genuine no-op and is skipped.
                // Substituting the whole field type rather than only a bare
                // parameter is what reaches an element type nested inside a
                // collection field. The shared bare-name thunk (`inst_args =
                // None`) is only reached as a collection element's decref
                // helper; there the direct drop already routed through the
                // mangled thunk, so an unresolvable generic field is skipped.
                let resolved =
                    crate::mir::lowering::inherited_instantiation::instantiated_field_types(
                        type_ctx.type_definitions,
                        type_name,
                        inst_args.unwrap_or_default(),
                    );
                let mut managed_fields: Vec<(usize, TypeKind)> = Vec::new();
                for (idx, field_ty) in resolved.iter().enumerate() {
                    let kind = &field_ty.kind;
                    if class_def.generics.is_some()
                        && Self::is_unresolved_generic_elem(kind, type_ctx.type_definitions)
                    {
                        continue;
                    }
                    if is_word_slot_managed(kind) {
                        managed_fields.push((idx, kind.clone()));
                    }
                }
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

    /// The index and resolved kind of every field of `struct_def` the drop path
    /// must DecRef.
    ///
    /// A field of a generic struct is written in the struct's own parameters
    /// (`value T`, `items List<T>`), which name nothing concrete on their own.
    /// The per-instantiation thunk supplies `inst_args`, so each field resolves
    /// to the kind this instance actually stores: a managed one joins the DecRef
    /// set at that kind, a scalar one is a genuine no-op and is skipped. The
    /// shared bare-name thunk passes no arguments and is only reached as a
    /// collection element's decref helper, where the direct drop already routed
    /// through the mangled thunk — so a field still written at a parameter is
    /// skipped rather than released at a type it may not have.
    fn managed_struct_fields(
        struct_def: &crate::type_checker::context::StructDefinition,
        inst_args: Option<&[Type]>,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Vec<(usize, TypeKind)> {
        let subs = Self::generic_substitution(struct_def.generics.as_deref(), inst_args);
        let mut managed = Vec::new();
        for (idx, (_, declared, _)) in struct_def.fields.iter().enumerate() {
            let resolved = match &subs {
                Some(subs) => crate::mir::lowering::apply_generic_sub(declared, subs),
                None => declared.clone(),
            };
            if struct_def.generics.is_some()
                && Self::is_unresolved_generic_elem(&resolved.kind, type_definitions)
            {
                continue;
            }
            if is_word_slot_managed(&resolved.kind) {
                managed.push((idx, resolved.kind));
            }
        }
        managed
    }

    /// Map each declared type parameter to the argument at its position, or
    /// `None` when there are no parameters, no arguments, or the two disagree
    /// about how many there are.
    fn generic_substitution(
        generics: Option<&[crate::type_checker::context::GenericDefinition]>,
        inst_args: Option<&[Type]>,
    ) -> Option<HashMap<String, Type>> {
        let generics = generics?;
        let args = inst_args?;
        if generics.len() != args.len() {
            return None;
        }
        Some(
            generics
                .iter()
                .zip(args)
                .map(|(param, arg)| (param.name.clone(), arg.clone()))
                .collect(),
        )
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
    ///
    /// For generic enums, `inst_args` provides the concrete type arguments so
    /// that generic variant fields can be resolved to their concrete kinds. A
    /// bare-generic field (`value T`) in a generic-enum variant is resolved to
    /// the argument at the parameter's declared position: a managed one (like
    /// `String`) joins the DecRef set at that kind, a scalar one (like `float`)
    /// is skipped. The shared bare-name thunk (inst_args = None) is only reached
    /// as a collection element's decref helper; there the direct drop already
    /// routed through the mangled thunk, so an unresolvable generic field is
    /// skipped here.
    fn emit_enum_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        enum_def: &EnumDefinition,
        inst_args: Option<&[Type]>,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let disc = builder
            .ins()
            .load(ptr_type, MemFlags::new(), payload_ptr, 0);
        // The drop thunk carries the instantiation as types; the layout
        // authority reads it as the type-argument expressions a construction
        // site carries, so both sides resolve payloads by the same rule.
        let type_args: Option<Vec<Expression>> =
            inst_args.map(|args| args.iter().cloned().map(type_expr_non_null).collect());
        let slot_size = layout::enum_payload_slot_size(enum_def, type_args.as_deref(), ptr_type);

        for (variant_idx, managed_fields) in
            Self::enum_variants_with_managed_fields(enum_def, type_args.as_deref(), type_ctx)
        {
            Self::emit_enum_variant_drop_guard(
                builder,
                ctx,
                EnumDropSite {
                    disc,
                    payload_ptr,
                    slot_size: slot_size as i32,
                },
                variant_idx,
                &managed_fields,
                type_ctx,
            )?;
        }
        Ok(())
    }

    /// Collect `(variant_idx, [(field_idx, field_kind), ...])` for every
    /// enum variant carrying at least one managed field. Variants without
    /// managed fields are filtered out so the caller only emits guarded
    /// blocks when there is decref work to do.
    ///
    /// For generic enums, resolves generic-parameter fields to their concrete
    /// kinds through [`layout::enum_payload_field_kind`] using `type_args`, so
    /// that a managed type argument (like `String`) is correctly identified as
    /// managed. A parameter left unresolved — the shared bare-name thunk has no
    /// arguments — is skipped.
    pub fn enum_variants_with_managed_fields(
        enum_def: &EnumDefinition,
        type_args: Option<&[Expression]>,
        type_ctx: &TypeCtx,
    ) -> Vec<(usize, Vec<(usize, TypeKind)>)> {
        enum_def
            .variants
            .iter()
            .enumerate()
            .filter_map(|(vi, (_name, fields))| {
                let managed: Vec<(usize, TypeKind)> = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(fi, ty)| {
                        let kind = layout::enum_payload_field_kind(enum_def, &ty.kind, type_args);
                        let unresolved = enum_def.generics.is_some()
                            && Self::is_unresolved_generic_elem(&kind, type_ctx.type_definitions);
                        (!unresolved && is_word_slot_managed(&kind)).then_some((fi, kind))
                    })
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
    fn emit_enum_variant_drop_guard(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        site: EnumDropSite,
        variant_idx: usize,
        managed_fields: &[(usize, TypeKind)],
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let EnumDropSite {
            disc,
            payload_ptr,
            slot_size,
        } = site;
        let ptr_type = type_ctx.ptr_type;
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
            // Payload field `k` is enum field `k + 1`: the discriminant
            // occupies the first slot.
            let field_offset = (*field_idx as i32 + 1) * slot_size;
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
    /// type is a managed heap object (String, List, Array, Set, Map, user-defined
    /// class, Tuple, or Option).
    ///
    /// Called by `translate_collection_index_write` before the new value is stored
    /// so that overwriting an existing slot does not leak the old value.
    ///
    /// Routing: built-in collections / String use their per-shape runtime decref
    /// helper (the fast path); user classes call `__decref_TypeName`; Tuple,
    /// Option and function values (the only `ElementShape::Other` variants that
    /// `is_field_managed` reports as managed) inline through
    /// `emit_decref_value`, which dispatches to `emit_type_drop` and
    /// recursively releases nested managed fields. Primitive `Other` shapes are
    /// the explicit no-op branch.
    pub(crate) fn emit_managed_elem_decref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_addr: Value,
        elem_type_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
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
        if let ElementShape::UserClass(class_name) = shape {
            // Route a recorded generic-class instantiation (`Box<String>`) to its
            // per-instantiation `__decref_Box__String` wrapper so the concrete
            // managed field is released; other classes use the bare name.
            let symbol = Self::generic_drop_thunk_name_part(
                class_name,
                Self::custom_type_args(elem_type_kind),
                type_ctx,
            );
            let mut decref_name = String::with_capacity(9 + symbol.len());
            decref_name.push_str("__decref_");
            decref_name.push_str(&symbol);
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
            return Ok(());
        }
        // `Other` shapes that `is_field_managed` flags as managed (Tuple, Option,
        // function values)
        // are heap-allocated with an RC header. Route through the inline
        // decref-and-drop emitter so nested managed payloads are released.
        if is_field_managed(elem_type_kind) {
            let old_val = builder.ins().load(ptr_type, MemFlags::new(), elem_addr, 0);
            Self::emit_decref_value(builder, ctx, elem_type_kind, old_val, type_ctx)?;
        }
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
        Self::emit_release_check(builder, ctx, ptr)?;

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
    /// `type_args = None` generates the bare `__drop_TypeName`; `Some(args)`
    /// generates a per-instantiation thunk (`__drop_Box__String`) whose body
    /// resolves each bare-generic field against `args`. This function emits only
    /// the bare thunk's matching `__decref_TypeName` wrapper; the caller emits
    /// the per-instantiation `__decref_Box__String` wrapper separately (via
    /// `generate_decref_function`) so a `List<Box<String>>` element's runtime
    /// decref helper routes through the per-instantiation drop.
    pub(crate) fn generate_drop_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_args: Option<&[Type]>,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<crate::ast::types::Type>>>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let mangled = match type_args {
            Some(args) => mangle_class_instantiation(type_name, args),
            None => type_name.to_string(),
        };
        let mut func_name = String::with_capacity(7 + mangled.len());
        func_name.push_str("__drop_");
        func_name.push_str(&mangled);
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
            type_args,
            type_definitions,
            generic_class_instantiations,
            ptr_type,
            call_conv,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(func_name, e.to_string()))?;
        ctx.clear();

        if type_args.is_some() {
            // The per-instantiation `__decref_Box__String` wrapper is emitted by
            // the caller after this thunk is defined, keyed by the mangled name.
            return Ok(());
        }
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
        type_args: Option<&[Type]>,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<crate::ast::types::Type>>>,
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

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            type_definitions,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
            generic_class_instantiations,
        };

        if let Some(hook_name) = Self::resolve_drop_hook_name(type_name, type_definitions) {
            Self::call_user_drop_hook(
                &mut builder,
                &mut module_ctx,
                &hook_name,
                ptr,
                ptr_type,
                call_conv,
            )?;
        }

        Self::emit_struct_drop(
            &mut builder,
            &mut module_ctx,
            type_name,
            type_args,
            ptr,
            &type_ctx,
        )?;

        let header_ptr = builder.ins().iadd_imm(ptr, -ptr_size);
        Self::call_libc_free(&mut builder, &mut module_ctx, header_ptr)?;

        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Emit a call to the user-defined drop hook `hook_name(self, allocator)`
    /// declared by `fn drop(self)`. ABI mirrors a method body lowered with its
    /// receiver and no explicit parameters: `(self: ptr, allocator: ptr) -> void`,
    /// with a null allocator placeholder.
    fn call_user_drop_hook(
        builder: &mut FunctionBuilder,
        module_ctx: &mut ModuleCtx,
        hook_name: &str,
        self_ptr: Value,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let mut user_sig = Signature::new(call_conv);
        user_sig.params.push(AbiParam::new(ptr_type)); // self
        user_sig.params.push(AbiParam::new(ptr_type)); // allocator
        let user_drop_id = module_ctx
            .module
            .declare_function(hook_name, Linkage::Import, &user_sig)
            .map_err(|e| CodegenError::declare_function(hook_name.to_string(), e.to_string()))?;
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
    pub(crate) fn generate_decref_function(
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

    /// Generates `__decref_{symbol}(ptr)` for a structural type — a tuple, an
    /// option or a function value, which carries managed payload but has no
    /// declaration whose name could be mangled into a symbol. `symbol` comes from
    /// [`crate::codegen::cranelift::structural_keys::structural_thunk_symbol`],
    /// which encodes the type's structure so distinct types get distinct thunks.
    ///
    /// The body is the same inline decref-and-drop sequence a direct release
    /// emits, so the structural payload is released exactly as it would be at a
    /// binding's scope exit. Used as a map's `key_drop_fn`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_structural_decref_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        symbol: &str,
        kind: &TypeKind,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<Type>>>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let mut decref_name = String::with_capacity(9 + symbol.len());
        decref_name.push_str("__decref_");
        decref_name.push_str(symbol);
        let mut sig = Signature::new(isa.default_call_conv());
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&decref_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_structural_decref_body(
            module,
            ctx,
            &mut builder_ctx,
            kind,
            type_definitions,
            generic_class_instantiations,
            ptr_type,
        )?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(decref_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of a structural decref thunk: release `ptr` at `kind`,
    /// which drops and frees it when its count reaches zero, then return.
    fn emit_structural_decref_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        kind: &TypeKind,
        type_definitions: &HashMap<String, TypeDefinition>,
        generic_class_instantiations: &HashMap<String, Vec<Vec<Type>>>,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            type_definitions,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
            generic_class_instantiations,
        };

        Self::emit_decref_value(&mut builder, &mut module_ctx, kind, ptr, &type_ctx)?;
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Emit the body of `__decref_TypeName`: null guard → heap-guard release
    /// check → immortal guard → decrement RC → when RC hits zero call
    /// `__drop_TypeName(ptr)` → return.
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

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);

        // Null guard.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        // Report the release to the heap guard, then load RC + check immortal
        // flag (high bit set).
        builder.switch_to_block(rc_block);
        builder.seal_block(rc_block);
        Self::emit_release_check(&mut builder, &mut module_ctx, ptr)?;
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

        builder.switch_to_block(free_block);
        builder.seal_block(free_block);
        Self::call_type_drop(&mut builder, module_ctx.module, type_name, sig, ptr)?;
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Emit the call `__drop_{type_name}(ptr)`, which runs the type's drop hook,
    /// releases its managed fields and frees it. `sig` is the drop thunk's
    /// `(ptr) -> void` signature.
    fn call_type_drop(
        builder: &mut FunctionBuilder,
        module: &mut ObjectModule,
        type_name: &str,
        sig: &Signature,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let mut drop_name = String::with_capacity(7 + type_name.len());
        drop_name.push_str("__drop_");
        drop_name.push_str(type_name);
        let drop_func_id = module
            .declare_function(&drop_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(drop_name.clone(), e.to_string()))?;
        let local_drop = module.declare_func_in_func(drop_func_id, builder.func);
        builder.ins().call(local_drop, &[ptr]);
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

    /// Resolves the symbol of the drop hook that releasing a `type_name` value
    /// runs, or `None` when the type has none.
    ///
    /// The owner is found by the same resolution an inherited method call uses,
    /// so a subclass reaches its base's hook (or its own copy, when the base is
    /// abstract) and the hook this thunk declares is the method body lowered for
    /// it. A struct is not a class chain and names its own hook.
    ///
    /// TODO: a class that declares `fn drop(self)` and no fields never runs the
    /// hook — `let h = Handle()` leaving scope prints nothing, and neither does
    /// `h.drop()`. A class with one field runs it. Where the fieldless instance
    /// loses its release (allocation, the managed-type predicate, or this thunk)
    /// is not yet traced.
    ///
    /// TODO: the hook is resolved from the static type of the reference being
    /// released, so a resource released through a trait-typed reference
    /// (`let c Closable = Handle(id: 1)`, or a temporary passed as a `Closable`
    /// argument) never runs its hook, and `x.drop()` on a trait-typed receiver
    /// calls the hook as a method and lets the owner's release run it again.
    pub fn resolve_drop_hook_name(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        if !crate::type_checker::utils::has_drop_hook(type_name, type_definitions) {
            return None;
        }
        let owner = crate::mir::lowering::dispatch::resolve_inherited_method(
            type_definitions,
            type_name,
            DROP_HOOK_NAME,
        )
        .map_or_else(|| type_name.to_string(), |(defining, _)| defining);
        Some(format!("{owner}_{DROP_HOOK_NAME}"))
    }

    /// Resolves the mangled name of the `clone()` method for `type_name`.
    ///
    /// Walks the inheritance chain to find where `clone()` is defined.  The
    /// concrete-caller / abstract-definer rule is applied: if the defining class
    /// is abstract, the caller's name is used instead (matching how
    /// `resolve_inherited_method` in `mir::lowering::dispatch` mangles the call).
    pub fn resolve_clone_method_name(
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

    /// Returns true if `type_name` (or any ancestor class) implements `Cloneable`,
    /// directly or through a trait that extends it.
    pub fn class_implements_cloneable(
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        Self::class_implements(
            type_name,
            crate::ast::types::CLONEABLE_TRAIT_NAME,
            type_definitions,
        )
    }

    /// Returns true if `type_name` or any class it extends implements
    /// `trait_name`, directly or through a trait that extends it.
    pub fn class_implements(
        type_name: &str,
        trait_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        crate::type_checker::context::class_implements_trait(
            type_name,
            trait_name,
            type_definitions,
        )
    }
}
